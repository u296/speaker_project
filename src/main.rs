use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    },
    time::Duration,
};

use clap::Parser;

use device::Device;
use midly::{Smf, TrackEvent};
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
}

struct Event {
    delta: u32,
    kind: Option<EventKind>,
}
enum EventKind {
    NoteUpdate { key: u8, vel: u8 },
    TempoUpdate(u32),
}

fn convert<'a, I: IntoIterator<Item = &'a TrackEvent<'a>>, B: FromIterator<Event>>(track: I) -> B {
    track
        .into_iter()
        .map(|track_event| Event {
            delta: track_event.delta.into(),
            kind: match track_event.kind {
                midly::TrackEventKind::Midi { channel, message } => match message {
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
                midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                    Some(EventKind::TempoUpdate(t.into()))
                }
                _ => None,
            },
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (file_path, baud_rate, allowed_channels, playlist_order) = {
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
        )
    };

    let file_buf = std::fs::read(&file_path)?;

    let midi_file = Smf::parse(&file_buf)?;

    let (ticks_per_beat, tick): (u32, Duration) = midi::deduce_timing(&midi_file.header.timing);

    println!("file contains {} track(s)", midi_file.tracks.len());

    if playlist_order.is_empty() {
        println!("no playlist was specified. Quitting");
        std::process::exit(0);
    }

    let device = Arc::new(Mutex::new(device::Device::new(baud_rate)?));

    let playlist = playlist_order
        .iter()
        .map(|x| convert::<_, Vec<_>>(&midi_file.tracks[*x]))
        .collect::<Vec<_>>();

    let tick_microseconds = Arc::new(AtomicU32::from(tick.as_micros() as u32));

    let f = futures::future::join_all(playlist.into_iter().map(|track| {
        tokio::task::spawn(play_track(
            track,
            ticks_per_beat,
            tick_microseconds.clone(),
            device.clone(),
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
    ticks_per_beat: u32,
    tick_microseconds: Arc<AtomicU32>,
    device: Arc<Mutex<Device>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut last_cycle_duration = Duration::from_secs(0);
    for track_event in track {
        tokio::time::sleep(
            (tick_microseconds.load(Ordering::SeqCst) * MICROSECOND * track_event.delta)
                .saturating_sub(last_cycle_duration),
        )
        .await;
        let cycle_begin = tokio::time::Instant::now();
        match track_event.kind {
            Some(e) => match e {
                EventKind::NoteUpdate { key, vel } => {
                    let mut device_lock = device.lock().await;
                    device_lock.transmit_message_async(key, vel).await?;
                }
                EventKind::TempoUpdate(t) => {
                    let us_per_beat = t;
                    let us_per_tick = us_per_beat / ticks_per_beat;

                    tick_microseconds.store(us_per_tick, Ordering::SeqCst);
                    println!("tick is now {} µs", us_per_tick);
                }
            },
            _ => (), /*midly::TrackEventKind::Midi { channel, message } => {
                         // TODO allowed channel check
                         match message {
                             midly::MidiMessage::NoteOff { key, vel } => {
                                 let mut device_lock = device.lock().await;
                                 device_lock.transmit_message_async(key.into(), 0).await?;
                             }
                             midly::MidiMessage::NoteOn { key, vel } => {
                                 let mut device_lock = device.lock().await;
                                 device_lock
                                     .transmit_message_async(key.into(), vel.into())
                                     .await?;
                             }
                             _ => (),
                         }
                     }
                     midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                         let us_per_beat: u32 = t.into();
                         let tick_us = us_per_beat / ticks_per_beat;

                         tick_microseconds.store(tick_us, Ordering::SeqCst);
                         println!("tick is now {} µs", tick_us);
                     }
                     _ => (),*/
        }
        let cycle_end = tokio::time::Instant::now();
        last_cycle_duration = cycle_end - cycle_begin;
    }
    Ok(())
}
