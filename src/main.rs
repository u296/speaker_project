use crate::device::Device;
use midi::MidiSequence;
use play::{play_track, InstrumentCount};
use std::{
    process::exit,
    sync::{Arc, Weak},
};
use tokio::sync::{broadcast, Barrier, Mutex};

mod args;
mod device;
mod midi;
mod play;

#[cfg(all(feature = "single-thread", feature = "multi-thread"))]
compile_error!("single-thread and multi-thread are mutually exclusive features");

#[cfg(feature = "multi-thread")]
type DeviceMutex = Mutex<dyn Device + Send + Sync>;
#[cfg(feature = "single-thread")]
type DeviceMutex = Mutex<dyn Device + Send>;

/* message format sent to device
big endian transmission format
first byte: message type
0x01 : tone update

tone update message layout
01 xx xx yy 01

x: u16 tone
y: u16 velocity
 */

fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "single-thread")]
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    #[cfg(feature = "multi-thread")]
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    rt.block_on(async_main()).unwrap();

    Ok(())
}

async fn async_main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args = args::Args::parse();

    let midi_sequence = MidiSequence::parse_file(
        &args.file_path,
        args.tracks.map(|x| x.into_iter()),
        args.initial_tick,
        args.list,
    )
    .await?;

    let device = device::new(args.baud_rate, args.dry_run)?;

    let instrument_count = Arc::new(Mutex::new(InstrumentCount { current: 0, max: 0 }));

    let barrier = Arc::new(Barrier::new(midi_sequence.tracks.len()));
    let (sender, _) = broadcast::channel(8);

    tokio::spawn(handle_ctrlc(Arc::downgrade(&device)));

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

async fn handle_ctrlc(
    device: Weak<DeviceMutex>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tokio::signal::ctrl_c().await?;

    if let Some(arc) = device.upgrade() {
        let mut device_lock = arc.lock().await;
        device_lock.reset().await?;

        exit(0);
    }

    Ok(())
}
