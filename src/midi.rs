use std::{path::Path, process::exit, time::Duration};

use midly::{MetaMessage, Smf, TrackEvent, TrackEventKind};

#[derive(Debug, Clone, Copy)]
pub struct Timing {
    pub ticks_per_beat: u32,
    pub tick: Duration,
}

pub fn deduce_timing(timing: &midly::Timing, initial_tick: Option<Duration>) -> Timing {
    match timing {
        midly::Timing::Metrical(a) => {
            println!("timing = metrical: {a}");

            let ticks_per_beat = <midly::num::u15 as Into<u16>>::into(*a).into();

            println!("ticks per beat: {ticks_per_beat}");
            if let Some(override_tick) = initial_tick {
                println!("using provided tick: {} µs", override_tick.as_micros());

                Timing {
                    ticks_per_beat,
                    tick: override_tick,
                }
            } else {
                let assumed_tick = Duration::from_micros(500);
                println!("assuming initial tick: {} µs", assumed_tick.as_micros());

                Timing {
                    ticks_per_beat,
                    tick: assumed_tick,
                }
            }
        }
        midly::Timing::Timecode(fps, subframe) => {
            println!("timing = timecode: {}, {}", fps.as_int(), subframe);

            let ticks_per_beat = *subframe as u32;
            let tick = Duration::from_micros(1000000 / (fps.as_int() as u64 * *subframe as u64));

            println!("ticks per beat: {ticks_per_beat}");
            if let Some(override_tick) = initial_tick {
                println!(
                    "found initial tick: {} µs but using provided tick of {} µs",
                    tick.as_micros(),
                    override_tick.as_micros()
                );
                Timing {
                    ticks_per_beat,
                    tick: override_tick,
                }
            } else {
                println!("initial tick: {} µs", tick.as_micros());

                Timing {
                    ticks_per_beat,
                    tick,
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Event {
    pub delta: u32,
    pub kind: Option<EventKind>,
}

#[derive(Debug, Clone)]
pub enum EventKind {
    NoteUpdate { key: u8, vel: u8 },
    TempoUpdate(u32),
    TrackName(String),
    TrackInstrument(String),
}

pub fn convert<'a, I: IntoIterator<Item = &'a TrackEvent<'a>>, B: FromIterator<Event>>(
    track: I,
) -> B {
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

fn get_track_name_raw<'a, I: Iterator<Item = &'a TrackEvent<'a>>>(track: I) -> Option<String> {
    for i in track {
        if let TrackEventKind::Meta(MetaMessage::TrackName(name_slice)) = i.kind {
            return Some(String::from_utf8_lossy(name_slice).to_string());
        }
    }
    None
}

fn get_track_instrument_raw<'a, I: Iterator<Item = &'a TrackEvent<'a>>>(
    track: I,
) -> Option<String> {
    for i in track {
        if let TrackEventKind::Meta(MetaMessage::InstrumentName(name_slice)) = i.kind {
            return Some(String::from_utf8_lossy(name_slice).to_string());
        }
    }
    None
}

pub struct MidiSequence {
    pub timing: Timing,
    pub tracks: Vec<Vec<Event>>,
}

impl MidiSequence {
    pub async fn parse_file(
        path: impl AsRef<Path>,
        track_indices: impl IntoIterator<Item = usize>,
        initial_tick: Option<Duration>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let file_buf = tokio::fs::read(path).await?;

        let raw_midi = Smf::parse(&file_buf)?;

        let timing = deduce_timing(&raw_midi.header.timing, initial_tick);

        println!(
            "file contains {} track(s), listing...",
            raw_midi.tracks.len()
        );

        for (i, raw_track) in raw_midi.tracks.iter().enumerate() {
            let name = get_track_name_raw(raw_track.iter()).unwrap_or_else(|| "Unknown".into());
            let instrument =
                get_track_instrument_raw(raw_track.iter()).unwrap_or_else(|| "Unknown".into());

            println!("{i:<2} - name: {name:<32} - instrument: {instrument}");
        }

        let play_tracks = track_indices
            .into_iter()
            .map(|n| convert::<_, Vec<_>>(raw_midi.tracks[n].iter()))
            .collect::<Vec<_>>();

        if play_tracks.is_empty() {
            println!("no tracks specified. Quitting");
            exit(0);
        }

        // no postprocessing, all tracks including tempo track will start at the same time

        Ok(Self {
            tracks: play_tracks,
            timing,
        })
    }
}
