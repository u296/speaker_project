use args::Speed;
use device::Device;
use midi::{Event, MidiSequence, Timing};
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, Barrier, Mutex},
    time::Instant,
};

mod args;
mod device;
mod midi;

/* message format sent to device
big endian transmission format
first byte: message type
0x01 : tone update
0x02 :

tone update message layout
01 xx xx yy

x: u16 tone
y: u16 velocity
 */

// c5 = 72
fn key_to_frequency(key: u8) -> f64 {
    let note = key as usize % 12;
    let octave = key as i32 / 12;

    /* notes modulus
    0 C
    1 C#
    2 D
    3 D#
    4 E
    5 F
    6 F#
    7 G
    8 G#
    9 A
    10 A#
    11 B
     */

    let octave_8_freqs = [
        4186.0, 4434.0, 4699.0, 4978.0, 5274.0, 5588.0, 5920.0, 6272.0, 6645.0, 7040.0, 7459.0,
        7902.0,
    ];

    octave_8_freqs[note] / 2.0f64.powi(8 - octave)
}

struct InstrumentCount {
    current: usize,
    max: usize,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = args::Args::parse();

    let midi_sequence =
        MidiSequence::parse_file(&args.file_path, args.tracks.iter().copied()).await?;

    let device = device::new(args.baud_rate, args.dry_run)?;

    let instrument_count = Arc::new(Mutex::new(InstrumentCount { current: 0, max: 0 }));

    let barrier = Arc::new(Barrier::new(args.tracks.len()));
    let (sender, _) = broadcast::channel(8);

    let f = futures::future::join_all(midi_sequence.tracks.into_iter().map(|track| {
        tokio::task::spawn(play_track(
            track,
            midi_sequence.timing,
            device.clone(),
            instrument_count.clone(),
            args.speed,
            barrier.clone(),
            sender.clone(),
        ))
    }));

    for i in f.await {
        match i? {
            Ok(_) => (),
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

static MICROSECOND: Duration = Duration::from_micros(1);

async fn play_track<I: IntoIterator<Item = Event>>(
    track: I,
    timing: Timing,
    device: Arc<Mutex<dyn Device + Send + Sync>>,
    instrument_count: Arc<Mutex<InstrumentCount>>,
    speed: Speed,
    start_barrier: Arc<Barrier>,
    tick_update_tx: broadcast::Sender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    start_barrier.wait().await;

    let ticks_per_beat = timing.ticks_per_beat;
    let mut tick = timing.tick.as_micros() as u32;

    let mut tick_update_rx = tick_update_tx.subscribe();

    let mut next_time = Instant::now();

    for track_event in track {
        next_time += track_event.delta * tick * MICROSECOND;

        let start_wait = Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(next_time) => {
                    break;
                },
                Ok(new_tick) = tick_update_rx.recv() => {
                    let now = Instant::now();
                    let elapsed_old_ticks = (now - start_wait).as_micros() as f64 / tick as f64;

                    let elapsed_old_ticks = elapsed_old_ticks.round() as u32;
                    let remaining_new_ticks = track_event.delta - elapsed_old_ticks;

                    if new_tick > tick {
                        next_time += Duration::from_micros(remaining_new_ticks as u64 * (new_tick - tick) as u64)
                    } else {
                        next_time -= Duration::from_micros(remaining_new_ticks as u64 * (tick - new_tick) as u64)
                    }

                    tick = new_tick;
                }
            }
        }

        if let Some(e) = track_event.kind {
            match e {
                midi::EventKind::NoteUpdate { key, vel } => {
                    let mut device_lock = device.lock().await;

                    device_lock
                        .transmit_message_async((key_to_frequency(key) * speed.pitch) as u16, vel)
                        .await?;

                    /*
                    I'm unsure exactly why this is needed, but without
                    it the timing of the notes goes apeshit. I suspect
                    it has to do with tokio::sleep_until only having
                    millisecond granularity
                     */
                    tokio::time::sleep(Duration::from_nanos(1)).await;

                    let mut instrument_count_lock = instrument_count.lock().await;
                    if vel != 0 {
                        instrument_count_lock.current += 1;

                        if instrument_count_lock.current > instrument_count_lock.max {
                            instrument_count_lock.max = instrument_count_lock.current;
                            println!("new maximum notes: {}", instrument_count_lock.max);
                        }
                    } else {
                        instrument_count_lock.current -= 1;
                    }
                }
                midi::EventKind::TempoUpdate(t) => {
                    let us_per_beat = t;
                    let us_per_tick = (us_per_beat as f64 / (ticks_per_beat as f64)) as u32;

                    let us_per_tick_tempo_adjusted =
                        (us_per_beat as f64 / (ticks_per_beat as f64 * speed.tempo)) as u32;

                    tick = us_per_tick_tempo_adjusted;
                    drop(tick_update_rx);
                    tick_update_tx.send(us_per_tick_tempo_adjusted)?;
                    tick_update_rx = tick_update_tx.subscribe();

                    println!(
                        "tick is now {us_per_tick_tempo_adjusted} µs, adjusted from {us_per_tick} µs"
                    );
                }
                _ => (),
            }
        }
    }
    Ok(())
}
