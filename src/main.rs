use std::{path::PathBuf, time::Duration};

use clap::Parser;

use midly::Smf;

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

    #[arg(long, num_args = 0..)]
    channels: Vec<u8>,

    #[arg(long, num_args = 1..)]
    tracks: Vec<usize>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
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

    let (ticks_per_beat, mut tick): (u64, Duration) = midi::deduce_timing(&midi_file.header.timing);

    println!("file contains {} track(s)", midi_file.tracks.len());

    if playlist_order.is_empty() {
        println!("no playlist was specified. Quitting");
        std::process::exit(0);
    }

    let mut device = device::Device::new(baud_rate)?;

    let playlist: Vec<_> = playlist_order.iter().map(|x| &midi_file.tracks[*x]).collect();

    for track in playlist.iter() {
        for trackevent in track.iter() {
            std::thread::sleep(tick * trackevent.delta.into());
            match trackevent.kind {
                midly::TrackEventKind::Midi { channel, message } => {
                    if allowed_channels.contains(&channel.into()) {
                        match message {
                            midly::MidiMessage::NoteOff { key, vel: _ } => {
                                device.transmit_message(key.into(), 0)?;
                            }
                            midly::MidiMessage::NoteOn { key, vel } => {
                                device.transmit_message(key.into(), vel.into())?;
                            }
                            _ => (),
                        }
                    }
                }
                midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(t)) => {
                    // t microseconds per beat

                    tick = std::time::Duration::from_micros(
                        <midly::num::u24 as Into<u32>>::into(t) as u64 / ticks_per_beat,
                    );

                    println!("tick is now {} µs", tick.as_micros());
                }

                _ => (),
            }
        }
    }

    Ok(())
}
