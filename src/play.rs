use std::{sync::Arc, time::Duration};

use tokio::{
    sync::{broadcast, Barrier, Mutex},
    time::Instant,
};

use crate::{args::Speed, device::Device, midi::Event};

#[derive(Debug, Clone, Copy)]
pub struct InstrumentCount {
    pub current: usize,
    pub max: usize,
}

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

async fn sleep_until(
    wakeup_time: &mut Instant,
    mut remaining_ticks: u32,
    tick_us: &mut u32,
    tick_update_rx: &mut broadcast::Receiver<u32>,
) {
    loop {
        let start_wait = Instant::now();
        tokio::select! {
            _ = tokio::time::sleep_until(*wakeup_time) => {
                break;
            },
        Ok(new_tick_us) = tick_update_rx.recv() => {
            let now = Instant::now();
            let elapsed_time = now - start_wait;
            let elapsed_old_ticks = (elapsed_time.as_secs_f64() * 1_000_000.0) / *tick_us as f64;

            let completed_old_ticks = elapsed_old_ticks.round() as u32;
            remaining_ticks = remaining_ticks.saturating_sub(completed_old_ticks);

            if new_tick_us > *tick_us {
                *wakeup_time += Duration::from_micros((remaining_ticks * (new_tick_us - *tick_us)).into());
            } else {
                *wakeup_time -= Duration::from_micros((remaining_ticks * (*tick_us - new_tick_us)).into());
            }

            *tick_us = new_tick_us;
        }
        }
    }
}

async fn handle_note_update(
    device: Arc<Mutex<dyn Device + Send + Sync>>,
    key: u8,
    vel: u8,
    instrument_count: Arc<Mutex<InstrumentCount>>,
    pitch: f64,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut device_lock = device.lock().await;
    device_lock
        .tone_update((key_to_frequency(key) * pitch) as u16, vel)
        .await?;

    drop(device_lock);

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
    drop(instrument_count_lock);

    Ok(())
}

async fn handle_tempo_update(
    new_us_per_beat: u32,
    ticks_per_beat: u32,
    tempo: f64,
    tick_update_tx: &broadcast::Sender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let us_per_tick = new_us_per_beat as f64 / (ticks_per_beat as f64);
    let us_per_tick_tempo_adjusted = us_per_tick / tempo;

    tick_update_tx.send(us_per_tick_tempo_adjusted.round() as u32)?;

    println!("tick is now {us_per_tick_tempo_adjusted} µs, adjusted from {us_per_tick} µs");

    Ok(())
}

pub async fn play_track<I: IntoIterator<Item = Event>>(
    track: I,
    timing: crate::midi::Timing,
    device: Arc<Mutex<dyn Device + Send + Sync>>,
    instrument_count: Arc<Mutex<InstrumentCount>>,
    speed: Speed,
    start_barrier: Arc<Barrier>,
    tick_update_tx: broadcast::Sender<u32>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    start_barrier.wait().await;

    let ticks_per_beat = timing.ticks_per_beat;
    let mut tick_us = timing.tick.as_micros() as u32;

    let mut tick_update_rx = tick_update_tx.subscribe();

    let mut next_time = Instant::now();

    for track_event in track {
        next_time += Duration::from_micros((track_event.delta * tick_us).into());

        sleep_until(
            &mut next_time,
            track_event.delta,
            &mut tick_us,
            &mut tick_update_rx,
        )
        .await;

        if let Some(e) = track_event.kind {
            match e {
                crate::midi::EventKind::NoteUpdate { key, vel } => {
                    handle_note_update(
                        device.clone(),
                        key,
                        vel,
                        instrument_count.clone(),
                        speed.pitch,
                    )
                    .await?;
                }
                crate::midi::EventKind::TempoUpdate(new_us_per_beat) => {
                    handle_tempo_update(
                        new_us_per_beat,
                        ticks_per_beat,
                        speed.tempo,
                        &tick_update_tx,
                    )
                    .await?
                }
                _ => (),
            }
        }
    }
    Ok(())
}
