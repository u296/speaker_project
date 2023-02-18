use std::{io::Write, path::PathBuf, str::FromStr, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_serial::SerialStream;

fn read_input<T, ParseError, Parser: Fn(&str) -> Result<T, ParseError>, Filter: Fn(&T) -> bool>(
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

pub fn new(
    baud_rate: u32,
    dummy_device: bool,
) -> Result<Arc<Mutex<dyn Device + Send + Sync>>, Box<dyn std::error::Error + Send + Sync>> {
    if dummy_device {
        println!("using dummy device");
        Ok(Arc::new(Mutex::new(DummyDevice)))
    } else {
        Ok(Arc::new(Mutex::new(SerialDevice::new(baud_rate)?)))
    }
}

#[async_trait]
pub trait Device {
    async fn transmit_message_async(
        &mut self,
        frequency: u16,
        vel: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

pub struct SerialDevice(SerialStream);

impl SerialDevice {
    pub fn new(baud_rate: u32) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
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
                read_input("selection: ", FromStr::from_str, |n| *n < ports.len())?
            }
        };

        let dev_name = ports[selection].port_name.split('/').last().unwrap();
        let dev_path: PathBuf = ["/dev", dev_name].iter().collect();

        println!("selected device {dev_name}");

        println!("baudrate: {baud_rate}");
        println!("opening device at {}", dev_path.to_string_lossy());

        Ok(Self(SerialStream::open(&tokio_serial::new(
            dev_path.to_string_lossy(),
            baud_rate,
        ))?))
    }
}

#[async_trait]
impl Device for SerialDevice {
    async fn transmit_message_async(
        &mut self,
        freq: u16,
        vel: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let freq = freq.to_be_bytes();

        let message: [u8; 5] = [0x01, freq[0], freq[1], vel, 0x01];
        let mut num_timed_out = 1;
        loop {
            match <_ as tokio::io::AsyncWriteExt>::write_all(&mut self.0, &message).await {
                Ok(_) => return Ok(()),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::TimedOut => eprintln!("timed out {num_timed_out}"),
                    _ => return Err(Box::new(e)),
                },
            }
            num_timed_out += 1;
        }
    }
}

pub struct DummyDevice;

#[async_trait]
impl Device for DummyDevice {
    async fn transmit_message_async(
        &mut self,
        _: u16,
        _: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }
}
