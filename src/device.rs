use std::{path::PathBuf, str::FromStr};

use crate::util;

pub struct Device(Box<dyn tokio_serial::SerialPort>);

impl Device {
    pub fn new(baud_rate: u32) -> Result<Self, Box<dyn std::error::Error>> {
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

        Ok(Self(
            tokio_serial::new(dev_path.to_string_lossy(), baud_rate).open()?,
        ))
    }

    pub fn transmit_message(&mut self, key: u8, vel: u8) -> Result<(), Box<dyn std::error::Error>> {
        let freq = util::key_to_frequency(key).to_be_bytes();

        let message: [u8; 4] = [0x01, freq[0], freq[1], vel.into()];
        let mut i = 1;
        loop {
            match self.0.write_all(&message) {
                Ok(_) => return Ok(()),
                Err(e) => match e.kind() {
                    std::io::ErrorKind::TimedOut => eprintln!("timed out {}", i),
                    _ => return Err(e.into()),
                },
            }
            i += 1;
        }
    }
}
