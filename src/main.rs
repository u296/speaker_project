use std::{
    io::{Read, Stdin, Stdout, Write},
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use clap::Parser;

use midly::Smf;

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

fn read_input<T, ParseError, Parser: Fn(&str) -> Result<T, ParseError>, Filter: Fn(&T) -> bool>(
    stdin: &Stdin,
    stdout: &mut Stdout,
    prompt: &str,
    parse: Parser,
    accept: Filter,
) -> Result<T, Box<dyn std::error::Error>> {
    let mut s = String::new();
    loop {
        stdout.write_all(prompt.as_bytes())?;
        stdout.flush()?;

        stdin.read_line(&mut s)?;

        if let Ok(x) = parse(s.trim()) {
            if accept(&x) {
                break Ok(x);
            }
        }
        s.clear();
    }
}

// c5 = 72
fn key_to_frequency(key: u8) -> u16 {
    let note = key as usize % 12;
    let octave = key as u32 / 12;

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
        4186, 4434, 4699, 4978, 5274, 5588, 5920, 6272, 6645, 7040, 7459, 7902,
    ];

    octave_8_freqs[note] / 2u16.pow(8 - octave)
}

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
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();

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

    let mut file_buf = Vec::new();
    let mut file = std::fs::File::open(&file_path)?;
    file.read_to_end(&mut file_buf)?;

    let midi = Smf::parse(&file_buf)?;

    let ticks_per_beat: u64 = match midi.header.timing {
        midly::Timing::Metrical(a) => {
            let x = <midly::num::u15 as Into<u16>>::into(a).into();
            eprintln!("metrical: {}", x);
            x
        }
        midly::Timing::Timecode(_, _) => {
            eprintln!("timecode");
            10
        }
    };

    println!("file contains {} track(s)", midi.tracks.len());

    if playlist_order.is_empty() {
        println!("no playlist was specified. Quitting");
        std::process::exit(0);
    }

    let playlist: Vec<_> = playlist_order.iter().map(|x| &midi.tracks[*x]).collect();

    let mut tick = Duration::from_micros(500);

    let ports = tokio_serial::available_ports()?;

    println!("listing available serial ports...");

    ports
        .iter()
        .enumerate()
        .for_each(|(i, p)| println!("{}: {}", i, p.port_name.split('/').last().unwrap()));

    if ports.is_empty() {
        println!("no available serial ports");
        std::process::exit(1);
    }

    let selection: usize = {
        if ports.len() == 1 {
            0
        } else {
            read_input(&stdin, &mut stdout, "selection: ", FromStr::from_str, |n| {
                *n < ports.len()
            })?
        }
    };

    let dev_name = ports[selection].port_name.split('/').last().unwrap();
    let dev_path = format!("/dev/{}", dev_name);

    println!("selected device {}", dev_name);

    println!("baudrate: {}", baud_rate);
    println!("opening device at {}", dev_path);

    let mut device = tokio_serial::new(&dev_path, baud_rate).open()?;

    for track in playlist.iter() {
        for trackevent in track.iter() {
            std::thread::sleep(tick * trackevent.delta.into());
            match trackevent.kind {
                midly::TrackEventKind::Midi { channel, message } => {
                    if allowed_channels.contains(&channel.into()) {
                        match message {
                            midly::MidiMessage::NoteOff { key, vel: _ } => {
                                let freq = key_to_frequency(key.into()).to_be_bytes();

                                let message: [u8; 4] = [0x01, freq[0], freq[1], 0x00];

                                let mut i = 1;

                                loop {
                                    match device.write_all(&message) {
                                        Ok(_) => break,
                                        Err(e) => match e.kind() {
                                            std::io::ErrorKind::TimedOut => eprintln!("timed out {}", i),
                                            _ => Err(e)?,
                                        },
                                    }
                                    i += 1;
                                }
                            }
                            midly::MidiMessage::NoteOn { key, vel } => {
                                let freq = key_to_frequency(key.into()).to_be_bytes();

                                println!(
                                    "{}    {}",
                                    <midly::num::u7 as Into<u8>>::into(key),
                                    <midly::num::u7 as Into<u8>>::into(vel)
                                );

                                let message: [u8; 4] = [0x01, freq[0], freq[1], vel.into()];
                                let mut i = 1;
                                loop {
                                    match device.write_all(&message) {
                                        Ok(_) => break,
                                        Err(e) => match e.kind() {
                                            std::io::ErrorKind::TimedOut => eprintln!("timed out {}", i),
                                            _ => Err(e)?,
                                        },
                                    }
                                    i += 1;
                                }
                            }
                            _ => (),
                        }
                    }
                }
                midly::TrackEventKind::SysEx(_) => (),
                midly::TrackEventKind::Escape(_) => (),
                midly::TrackEventKind::Meta(m) => {
                    if let midly::MetaMessage::Tempo(t) = m {
                        // t microseconds per beat

                        tick = std::time::Duration::from_micros(
                            <midly::num::u24 as Into<u32>>::into(t) as u64 / ticks_per_beat,
                        );

                        println!("tick is now {} microseconds", tick.as_micros());
                    }
                }
            }
        }
    }

    Ok(())
}
