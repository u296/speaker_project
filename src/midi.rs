use std::time::Duration;

use midly::{Timing, TrackEvent};

pub fn deduce_timing(timing: &Timing) -> (u32, Duration) {
    match timing {
        midly::Timing::Metrical(a) => {
            println!("timing = metrical: {a}");

            let ticks_per_beat = <midly::num::u15 as Into<u16>>::into(*a).into();
            let tick = Duration::from_micros(500);

            println!("ticks per beat: {ticks_per_beat}");
            println!("assuming initial tick: {} µs", tick.as_micros());

            (ticks_per_beat, tick)
        }
        midly::Timing::Timecode(fps, subframe) => {
            println!("timing = timecode: {}, {}", fps.as_int(), subframe);

            let ticks_per_beat = *subframe as u32;
            let tick = Duration::from_micros(1000000 / (fps.as_int() as u64 * *subframe as u64));

            println!("ticks per beat: {ticks_per_beat}");
            println!("initial tick: {} µs", tick.as_micros());

            (ticks_per_beat, tick)
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

pub fn get_track_name<'a, I: IntoIterator<Item = &'a Event>>(i: I) -> Option<&'a str> {
    for event in i {
        if let Some(EventKind::TrackName(name)) = &event.kind {
            return Some(name);
        }
    }

    None
}

pub fn get_track_instrument<'a, I: IntoIterator<Item = &'a Event>>(i: I) -> Option<&'a str> {
    for event in i {
        if let Some(EventKind::TrackInstrument(name)) = &event.kind {
            return Some(name);
        }
    }

    None
}
