use clap::Parser;
use device::{Device, DummyDevice, SerialDevice};
use midly::{Smf, TrackEvent};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Mutex;

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
}

#[derive(Debug, Clone)]
struct Event {
    delta: u32,
    kind: Option<EventKind>,
}

#[derive(Debug, Clone)]
enum EventKind {
    NoteUpdate { key: u8, vel: u8 },
    TempoUpdate(u32),
    TrackName(String),
    TrackInstrument(String),
}

fn convert<'a, I: IntoIterator<Item = &'a TrackEvent<'a>>, B: FromIterator<Event>>(track: I) -> B {
    track
        .into_iter()
        .map(|track_event| Event {
            delta: track_event.delta.into(),
            kind: match track_event.kind {
                midly::TrackEventKind::Midi {
                    channel: _,
                    message,
                } => match message {
                    midly::MidiMessage::NoteOff { key, vel: _ } => Some(EventKind::NoteUpdate {
                        key: key.into(),
                        vel: 0,
                    }),
                    midly::MidiMessage::NoteOn { key, vel } => Some(EventKind::NoteUpdate {
                        key: key.into(),
                        vel: vel.into(),
                    }),
                    _ => None,
                },
                midly::TrackEventKind::Meta(m) => match m {
                    midly::MetaMessage::Tempo(t) => Some(EventKind::TempoUpdate(t.into())),
                    midly::MetaMessage::TrackName(bytes) => Some(EventKind::TrackName(
                        String::from_utf8_lossy(bytes).to_string(),
                    )),
                    midly::MetaMessage::InstrumentName(bytes) => Some(EventKind::TrackInstrument(
                        String::from_utf8_lossy(bytes).to_string(),
                    )),
                    _ => None,
                },
                _ => None,
            },
        })
        .collect()
}

fn get_track_name<'a, I: IntoIterator<Item = &'a Event>>(i: I) -> Option<&'a str> {
    for event in i {
        if let Some(EventKind::TrackName(name)) = &event.kind {
            return Some(name);
        }
    }

    None
}
fn get_track_instrument<'a, I: IntoIterator<Item = &'a Event>>(i: I) -> Option<&'a str> {
    for event in i {
        if let Some(EventKind::TrackInstrument(name)) = &event.kind {
            return Some(name);
        }
    }

    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (file_path, baud_rate, _allowed_channels, playlist_order, dummy_device) = {
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
        )
    };

    let file_buf = std::fs::read(&file_path)?;

    let midi_file = Smf::parse(&file_buf)?;

    let (ticks_per_beat, tick): (u32, Duration) = midi::deduce_timing(&midi_file.header.timing);

    println!(
        "file contains {} track(s), listing...",
        midi_file.tracks.len()
    );
    let tracks = midi_file
        .tracks
        .iter()
        .map(convert)
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

    let device: Arc<Mutex<dyn Device + Send + Sync>> = if dummy_device {
        println!("using dummy device");
        Arc::new(Mutex::new(DummyDevice))
    } else {
        Arc::new(Mutex::new(SerialDevice::new(baud_rate)?))
    };

    let tick_microseconds = Arc::new(AtomicU32::from(tick.as_micros() as u32));
    let current_instruments = Arc::new(Mutex::new(0));
    let max_instruments = Arc::new(Mutex::new(0));

    let f = futures::future::join_all(playlist_order.iter().map(|i| tracks[*i].clone()).map(
        |track| {
            tokio::task::spawn(play_track(
                track,
                ticks_per_beat,
                tick_microseconds.clone(),
                device.clone(),
                current_instruments.clone(),
                max_instruments.clone(),
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
    tick_microseconds: Arc<AtomicU32>,
    device: Arc<Mutex<dyn Device + Send + Sync>>,
    current_instruments: Arc<Mutex<u32>>,
    max_instruments: Arc<Mutex<u32>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut last_cycle_duration = Duration::from_secs(0);
    for track_event in track {
        tokio::time::sleep(
            (tick_microseconds.load(Ordering::SeqCst) * MICROSECOND * track_event.delta)
                .saturating_sub(last_cycle_duration),
        )
        .await;
        let cycle_begin = tokio::time::Instant::now();
        if let Some(e) = track_event.kind {
            match e {
                EventKind::NoteUpdate { key, vel } => {
                    let mut device_lock = device.lock().await;

                    device_lock.transmit_message_async(key, vel).await?;

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
                EventKind::TempoUpdate(t) => {
                    let us_per_beat = t;
                    let us_per_tick = us_per_beat / ticks_per_beat;

                    tick_microseconds.store(us_per_tick, Ordering::SeqCst);
                    println!("tick is now {us_per_tick} µs");
                }
                _ => (),
            }
        }
        let cycle_end = tokio::time::Instant::now();
        last_cycle_duration = cycle_end - cycle_begin;
    }
    Ok(())
}
