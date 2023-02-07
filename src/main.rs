use std::{
    io::{Read, Stdin, Stdout, Write},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use clap::Parser;

use midly::{Smf, Track};

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

fn read_integer<T: std::str::FromStr, F: Fn(&T) -> bool>(
    stdin: &Stdin,
    stdout: &mut Stdout,
    prompt: &str,
    accept: F,
) -> Result<T, Box<dyn std::error::Error>> {
    let mut s = String::new();
    loop {
        stdout.write_all(prompt.as_bytes())?;
        stdout.flush()?;

        stdin.read_line(&mut s)?;

        let b = s.trim();

        if let Ok(x) = b.parse::<T>() {
            if accept(&x) {
                break Ok(x);
            }
        }
        s.clear();
    }
}

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

fn read_input_multiple<
    T,
    ParseError,
    Parser: Fn(&str) -> Result<T, ParseError>,
    Filter: Fn(&T) -> bool,
>(
    stdin: &Stdin,
    stdout: &mut Stdout,
    prompt: &str,
    parse: Parser,
    allow: Filter,
) -> Result<Vec<T>, Box<dyn std::error::Error>> {
    let mut v = vec![];
    let mut s = String::new();
    loop {
        stdout.write_all(prompt.as_bytes())?;
        stdout.flush()?;
        stdin.read_line(&mut s)?;

        if s.trim().is_empty() {
            return Ok(v);
        }

        if let Ok(x) = parse(s.trim()) {
            if allow(&x) {
                v.push(x);
            } else {
                println!("not allowed, ignoring");
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

    /*let () = {
        let args = Args::parse();

        (filepath, baudrate, channels, tracks)
    };*/

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

    let baud_rate: u32 = 250000; /*read_input(
                                     &stdin,
                                     &mut stdout,
                                     "baud rate: ",
                                     FromStr::from_str,
                                     |_| true,
                                 )?;*/

    println!("assuming baudrate: {}", baud_rate);
    println!("opening device at {}", dev_path);

    let mut device = tokio_serial::new(&dev_path, baud_rate).open()?;

    let file_path: PathBuf = read_input(
        &stdin,
        &mut stdout,
        "file to play: ",
        FromStr::from_str,
        |x: &PathBuf| x.is_file(),
    )?;

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

    println!("select allowed channels (no input to allow all)");
    let allowed_channels: Vec<u8> =
        read_input_multiple(&stdin, &mut stdout, "channel: ", FromStr::from_str, |_| {
            true
        })?;

    let allowed_channels = {
        if allowed_channels.is_empty() {
            (0..=255).into_iter().collect()
        } else {
            allowed_channels
        }
    };

    println!("file contains {} track(s)", midi.tracks.len());
    println!("select order of tracks to play");
    let playlist: Vec<&Track> = read_input_multiple::<usize, _, _, _>(
        &stdin,
        &mut stdout,
        "track: ",
        FromStr::from_str,
        |x| *x < midi.tracks.len(),
    )?
    .iter()
    .map(|x| &midi.tracks[*x])
    .collect();

    let mut tick = Duration::from_micros(500);

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

                                loop {
                                    match device.write_all(&message) {
                                        Ok(_) => break,
                                        Err(e) => match e.kind() {
                                            std::io::ErrorKind::TimedOut => eprintln!("timed out"),
                                            _ => Err(e)?,
                                        },
                                    }
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
                                loop {
                                    match device.write_all(&message) {
                                        Ok(_) => break,
                                        Err(e) => match e.kind() {
                                            std::io::ErrorKind::TimedOut => println!("timed out"),
                                            _ => Err(e)?,
                                        },
                                    }
                                }
                            }
                            _ => (),
                        }
                    }
                }
                midly::TrackEventKind::SysEx(_) => (),
                midly::TrackEventKind::Escape(_) => (),
                midly::TrackEventKind::Meta(m) => match m {
                    midly::MetaMessage::Tempo(t) => {
                        println!("changing tempo to {}", t);

                        // t microseconds per beat

                        tick = std::time::Duration::from_micros(
                            <midly::num::u24 as Into<u32>>::into(t) as u64 / ticks_per_beat,
                        );

                        println!("tick is now {} microseconds", tick.as_micros());
                    }
                    _ => (),
                },
            }
        }
    }

    Ok(())
}
