use std::time::Duration;

use midly::Timing;

pub fn deduce_timing(timing: &Timing) -> (u32, Duration) {
    match timing {
        midly::Timing::Metrical(a) => {
            println!("timing = metrical: {}", a);

            let ticks_per_beat = <midly::num::u15 as Into<u16>>::into(*a).into();
            let tick = Duration::from_micros(500);

            println!("ticks per beat: {}", ticks_per_beat);
            println!("assuming initial tick: {} µs", tick.as_micros());

            (ticks_per_beat, tick)
        }
        midly::Timing::Timecode(fps, subframe) => {
            println!("timing = timecode: {}, {}", fps.as_int(), subframe);

            let ticks_per_beat = *subframe as u32;
            let tick = Duration::from_micros(1000000 / (fps.as_int() as u64 * *subframe as u64));

            println!("ticks per beat: {}", ticks_per_beat);
            println!("initial tick: {} µs", tick.as_micros());

            (ticks_per_beat, tick)
        }
    }
}
