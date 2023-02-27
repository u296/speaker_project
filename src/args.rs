use clap::{command, ArgGroup, Parser};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(group(
    ArgGroup::new("speed_components")
        .required(false)
        .multiple(true)
        .args(["pitch_shift", "tempo_shift"])
))]
struct RawArgs {
    file: PathBuf,
    #[arg(short, long, default_value_t = 250000)]
    baudrate: u32,

    #[arg(short = 't', long)]
    assume_initial_tick: Option<u64>,

    #[arg(long, num_args = 1..)]
    tracks: Option<Vec<usize>>,

    #[arg(short, long)]
    dry: bool,

    #[arg(
        long,
        allow_negative_numbers = true,
        conflicts_with("speed_components")
    )]
    speed_shift: Option<i8>,

    #[arg(long, allow_negative_numbers = true)]
    pitch_shift: Option<i8>,

    #[arg(long, allow_negative_numbers = true)]
    tempo_shift: Option<i8>,
}

fn delta_note_to_multiplier(delta: i8) -> f64 {
    2.0f64.powf(delta as f64 / 12.0)
}

#[derive(Debug, Clone, Copy)]
pub struct Speed {
    pub tempo: f64,
    pub pitch: f64,
}

#[derive(Debug, Clone)]
pub struct Args {
    pub file_path: PathBuf,
    pub baud_rate: u32,
    pub tracks: Option<Vec<usize>>,
    pub dry_run: bool,
    pub speed: Speed,
    pub initial_tick: Option<Duration>,
}

impl Args {
    pub fn parse() -> Args {
        let args = RawArgs::parse();

        let (pitch_multiplier, tempo_multiplier) = if let Some(speed_shift) = args.speed_shift {
            (
                delta_note_to_multiplier(speed_shift),
                delta_note_to_multiplier(speed_shift),
            )
        } else {
            (
                delta_note_to_multiplier(args.pitch_shift.unwrap_or(0)),
                delta_note_to_multiplier(args.tempo_shift.unwrap_or(0)),
            )
        };

        let speed = Speed {
            pitch: pitch_multiplier,
            tempo: tempo_multiplier,
        };

        Args {
            file_path: args.file,
            baud_rate: args.baudrate,
            tracks: args.tracks,
            dry_run: args.dry,
            speed,
            initial_tick: args.assume_initial_tick.map(Duration::from_micros),
        }
    }
}
