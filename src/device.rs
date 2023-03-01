use std::{io::Write, path::PathBuf, process::exit, str::FromStr, sync::Arc};

use async_trait::async_trait;
use tokio::sync::Mutex;
use tokio_serial::SerialStream;

use crate::DeviceMutex;

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

pub async fn new(
    baud_rate: u32,
    dummy_device: bool,
    ignore_id: bool,
) -> Result<Arc<DeviceMutex>, Box<dyn std::error::Error + Send + Sync>> {
    if dummy_device {
        println!("using dummy device");
        Ok(Arc::new(Mutex::new(DummyDevice)))
    } else {
        Ok(Arc::new(Mutex::new(
            SerialDevice::new(baud_rate, ignore_id).await?,
        )))
    }
}

#[async_trait]
pub trait Device {
    async fn tone_update(
        &mut self,
        frequency: u16,
        vel: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn reset(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    async fn verify_id(
        &mut self,
    ) -> Result<Result<(), [u8; 4]>, Box<dyn std::error::Error + Send + Sync>>;
}

const MAGIC_ID: [u8; 4] = [0x61, 0xd8, 0x6e, 0x1c];
pub struct SerialDevice(SerialStream);

impl SerialDevice {
    pub async fn new(
        baud_rate: u32,
        ignore_id: bool,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
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

        #[cfg(target_family = "unix")]
        let (dev_name, dev_path) = {
            let dev_name = ports[selection].port_name.split('/').last().unwrap();
            let dev_path: PathBuf = ["/dev", dev_name].iter().collect();

            (dev_name, dev_path)
        };
        #[cfg(target_family = "windows")]
        let (dev_name, dev_path) = {
            let dev_name = ports[selection].port_name.clone();
            let dev_path: PathBuf = dev_name.clone().into();

            (dev_name, dev_path)
        };

        println!("selected device {dev_name}");

        println!("baudrate: {baud_rate}");
        println!("opening device at {}", dev_path.to_string_lossy());

        let mut dev = Self(SerialStream::open(&tokio_serial::new(
            dev_path.to_string_lossy(),
            baud_rate,
        ))?);

        match dev.verify_id().await {
            Ok(r) => match r {
                Ok(_) => {
                    print!("device answered with correct ID: ");
                    for byte in MAGIC_ID.iter() {
                        print!("{:X}", *byte);
                    }
                    println!("");
                }
                Err(response) => {
                    print!("device answered with incorrect ID: ");
                    for byte in response.iter() {
                        print!("{:X}", *byte);
                    }
                    println!("");
                    if ignore_id {
                        println!("ignoring")
                    } else {
                        exit(1);
                    }
                }
            },
            Err(e) => {
                println!("device failed to answer ID: {e}");
                exit(1);
            }
        }

        Ok(dev)
    }
}

/* message format sent to device
big endian transmission format
first byte: message type
0x01 : tone update
0x02 : reset
0x03 : get id

tone update message layout
01 xx xx yy 01

x: u16 tone
y: u16 velocity

reset message layout

02

get id message layout

03

RESPONSE: 4 bytes

61 d8 6e 1c

 */

#[async_trait]
impl Device for SerialDevice {
    async fn tone_update(
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

    async fn reset(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let message: [u8; 1] = [0x2];

        <_ as tokio::io::AsyncWriteExt>::write_all(&mut self.0, &message)
            .await
            .map_err(|e| e.into())
    }

    async fn verify_id(
        &mut self,
    ) -> Result<Result<(), [u8; 4]>, Box<dyn std::error::Error + Send + Sync>> {
        let message: [u8; 1] = [0x3];

        let mut buf: [u8; 4] = [0; 4];

        <_ as tokio::io::AsyncWriteExt>::write_all(&mut self.0, &message).await?;
        <_ as tokio::io::AsyncReadExt>::read_exact(&mut self.0, &mut buf).await?;

        if buf == MAGIC_ID {
            Ok(Ok(()))
        } else {
            Ok(Err(buf))
        }
    }
}

pub struct DummyDevice;

#[async_trait]
impl Device for DummyDevice {
    async fn tone_update(
        &mut self,
        _: u16,
        _: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn reset(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Ok(())
    }

    async fn verify_id(
        &mut self,
    ) -> Result<Result<(), [u8; 4]>, Box<dyn std::error::Error + Send + Sync>> {
        Ok(Ok(()))
    }
}
