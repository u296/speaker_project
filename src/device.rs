use std::{io::Write, path::PathBuf, str::FromStr};

use tokio_serial::SerialStream;

use crate::util;

pub struct Device(SerialStream);

impl Device {
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
                util::read_input("selection: ", FromStr::from_str, |n| *n < ports.len())?
            }
        };

        let dev_name = ports[selection].port_name.split('/').last().unwrap();
        let dev_path: PathBuf = ["/dev", dev_name].iter().collect();

        println!("selected device {}", dev_name);

        println!("baudrate: {}", baud_rate);
        println!("opening device at {}", dev_path.to_string_lossy());

        Ok(Self(SerialStream::open(&tokio_serial::new(
            dev_path.to_string_lossy(),
            baud_rate,
        ))?))
    }

    pub async fn transmit_message_async(
        &mut self,
        key: u8,
        vel: u8,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let freq = util::key_to_frequency(key).to_be_bytes();

        let message: [u8; 4] = [0x01, freq[0], freq[1], vel];
        let mut i = 1;
        loop {
            match <_ as tokio::io::AsyncWriteExt>::write_all(&mut self.0, &message).await {
                Ok(_) => return Ok(()),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::TimedOut => eprintln!("timed out {}", i),
                    _ => return Err(Box::new(e)),
                },
            }
            i += 1;
        }
    }
}
