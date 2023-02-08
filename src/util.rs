use std::io::Write;

// 72 = C5
pub fn key_to_frequency(key: u8) -> f64 {
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

pub fn read_input<
    T,
    ParseError,
    Parser: Fn(&str) -> Result<T, ParseError>,
    Filter: Fn(&T) -> bool,
>(
    prompt: &str,
    parse: Parser,
    accept: Filter,
) -> Result<T, Box<dyn std::error::Error + Send + Sync>> {
    let mut s = String::new();
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
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
