use midi::MidiSequence;
use play::{play_track, InstrumentCount};
use std::sync::Arc;
use tokio::sync::{broadcast, Barrier, Mutex};

mod args;
mod device;
mod midi;
mod play;

/* message format sent to device
big endian transmission format
first byte: message type
0x01 : tone update

tone update message layout
01 xx xx yy 01

x: u16 tone
y: u16 velocity
 */

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = args::Args::parse();

    let midi_sequence = MidiSequence::parse_file(
        &args.file_path,
        args.tracks.map(|x| x.into_iter()),
        args.initial_tick,
    )
    .await?;

    let device = device::new(args.baud_rate, args.dry_run)?;

    let instrument_count = Arc::new(Mutex::new(InstrumentCount { current: 0, max: 0 }));

    let barrier = Arc::new(Barrier::new(midi_sequence.tracks.len()));
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
