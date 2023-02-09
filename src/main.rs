use clap::Parser;
use device::{Device, DummyDevice, SerialDevice};
use midi::{get_track_instrument, get_track_name, Event};
use midly::Smf;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    sync::{broadcast, Barrier, Mutex},
    time::Instant,
};

mod device;
mod midi;
mod util;

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

#[derive(Parser)]
struct Args {
    file: PathBuf,
    #[arg(short, long, default_value_t = 250000)]
    baudrate: u32,

    #[arg(long, num_args = 1..)]
    channels: Vec<u8>,

    #[arg(long, num_args = 1..)]
    tracks: Vec<usize>,

    #[arg(short, long)]
    dry: bool,

    #[arg(long, allow_negative_numbers = true, default_value_t = 0)]
    pitch_shift: i8,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (file_path, baud_rate, _allowed_channels, playlist_order, dummy_device, pitch_shift) = {
        let args = Args::parse();

        (
            args.file,
            args.baudrate,
            if args.channels.is_empty() {
                (0..=255).into_iter().collect()
            } else {
                args.channels
            },
            args.tracks,
            args.dry,
            args.pitch_shift,
        )
    };

    let file_buf = tokio::fs::read(&file_path).await?;

    let midi_file = Smf::parse(&file_buf)?;

    let (ticks_per_beat, tick): (u32, Duration) = midi::deduce_timing(&midi_file.header.timing);

    println!(
        "file contains {} track(s), listing...",
        midi_file.tracks.len()
    );
    let mut tracks = midi_file
        .tracks
        .iter()
        .map(midi::convert)
        .collect::<Vec<Vec<_>>>();

    for (i, track) in tracks.iter().enumerate() {
        let name = get_track_name(track).unwrap_or("Unknown");
        let instrument = get_track_instrument(track).unwrap_or("Unknown");

        println!("{i:<2} - name: {name:<40} - instrument: {instrument}");
    }

    if playlist_order.is_empty() {
        println!("no playlist was specified. Quitting");
        std::process::exit(0);
    }

    let mut tempo_track_index = None;

    'out: for (track_index, track) in tracks.iter().enumerate() {
        for event in track.iter() {
            if let Some(midi::EventKind::TempoUpdate(_)) = event.kind {
                println!("determined that track {track_index} is the tempo track");
                tempo_track_index = Some(track_index);
                break 'out;
            }
        }
    }

    if let Some(tempo_track_index) = tempo_track_index {
        for (track_index, track) in tracks.iter_mut().enumerate() {
            if track_index != tempo_track_index {
                track.insert(
                    0,
                    Event {
                        delta: 100,
                        kind: None,
                    },
                );
            }
        }
    } else {
        println!("unable to determine tempo track");
        std::process::exit(1);
    }

    let device: Arc<Mutex<dyn Device + Send + Sync>> = if dummy_device {
        println!("using dummy device");
        Arc::new(Mutex::new(DummyDevice))
    } else {
        Arc::new(Mutex::new(SerialDevice::new(baud_rate)?))
    };

    let freq_multiplier = 2.0f64.powf(pitch_shift as f64 / 12.0);
    let speed_multiplier = freq_multiplier;

    let current_instruments = Arc::new(Mutex::new(0));
    let max_instruments = Arc::new(Mutex::new(0));

    let barrier = Arc::new(Barrier::new(playlist_order.len()));
    let (sender, _) = broadcast::channel(8);

    let f = futures::future::join_all(playlist_order.iter().map(|i| tracks[*i].clone()).map(
        |track| {
            tokio::task::spawn(play_track(
                track,
                ticks_per_beat,
                tick.as_micros() as u32,
                device.clone(),
                current_instruments.clone(),
                max_instruments.clone(),
                freq_multiplier,
                speed_multiplier,
                barrier.clone(),
                sender.clone(),
            ))
        },
    ));

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
    ticks_per_beat: u32,
    mut tick: u32,
    device: Arc<Mutex<dyn Device + Send + Sync>>,
    current_instruments: Arc<Mutex<u32>>,
    max_instruments: Arc<Mutex<u32>>,
    freq_multiplier: f64,
    speed_multiplier: f64,
    start_barrier: Arc<Barrier>,
    bus_transmitter: broadcast::Sender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    start_barrier.wait().await;

    let mut listener = bus_transmitter.subscribe();

    let mut next_time = Instant::now();

    for track_event in track {
        next_time += (track_event.delta * tick * MICROSECOND).div_f64(speed_multiplier);

        let start_wait = Instant::now();
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(next_time) => {
                    break;
                },
                Ok(new_tick) = listener.recv() => {
                    let now = Instant::now();
                    let elapsed_old_ticks = (now - start_wait).as_micros() as f64 / tick as f64;

                    println!("interrupted: old tick = {tick} new tick = {new_tick}, elapsed old ticks: {elapsed_old_ticks}");

                    let elapsed_old_ticks = elapsed_old_ticks.round() as u32;
                    let remaining_new_ticks = track_event.delta - elapsed_old_ticks;

                    let next_time_delta = remaining_new_ticks as i32 * (tick as i32 - new_tick as i32);

                    println!("remaining ticks = {remaining_new_ticks} - shifting next by {} µs", next_time_delta);

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
                        .transmit_message_async(
                            (util::key_to_frequency(key) * freq_multiplier) as u16,
                            vel,
                        )
                        .await?;

                    /*
                    I'm unsure exactly why this is needed, but without
                    it the timing of the notes goes apeshit. I suspect
                    it has to do with tokio::sleep_until only having
                    millisecond granularity, and this operation finishing
                    too fast will mess things up
                     */
                    tokio::time::sleep(Duration::from_millis(1)).await;

                    if vel != 0 {
                        let mut current_instruments_lock = current_instruments.lock().await;
                        let mut max_instruments_lock = max_instruments.lock().await;

                        *current_instruments_lock += 1;

                        if *current_instruments_lock > *max_instruments_lock {
                            *max_instruments_lock = *current_instruments_lock;
                            println!("new maximum notes: {}", *max_instruments_lock);
                        }
                    } else {
                        *current_instruments.lock().await -= 1;
                    }
                }
                midi::EventKind::TempoUpdate(t) => {
                    let us_per_beat = t;
                    let us_per_tick =
                        (us_per_beat as f64 / (ticks_per_beat as f64 * speed_multiplier)) as u32;

                    println!("speed multiplier: {speed_multiplier}");

                    tick = us_per_tick;
                    drop(listener);
                    bus_transmitter.send(us_per_tick)?;
                    listener = bus_transmitter.subscribe();

                    println!("tick is now {us_per_tick} µs");
                }
                _ => (),
            }
        }
    }
    Ok(())
}
