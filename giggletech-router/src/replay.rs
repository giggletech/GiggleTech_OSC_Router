use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::time::{Instant, Duration};
use regex::Regex;
use std::sync::atomic::AtomicBool;
use async_std::task;
use async_std::sync::Arc;
use async_std::task::sleep;

mod config;
use config::*;
mod osc_timeout;
use osc_timeout::*;

mod data_processing;
mod giggletech_osc;
mod terminator;
mod handle_proximity_parameter;



#[derive(Debug)]
pub struct Packet {
    timestamp: Duration,
    axis: String,  // x+ x- y+ y- z+ z-
    amount: f32,
}

impl Packet {
    pub fn try_get_from_line(re: &Regex, line: &str) -> Option<Packet> {
        if let Some(caps) = re.captures(line) {
            let hours: u64 = caps["hours"].parse().ok()?;
            let minutes: u64 = caps["minutes"].parse().ok()?;
            let seconds: u64 = caps["seconds"].parse().ok()?;
            let millis: u64 = caps["millis"].parse().ok()?;

            let timestamp = Duration::from_secs(hours * 3600 + minutes * 60 + seconds) + Duration::from_millis(millis);
            let axis = caps["axis"].to_string();
            let amount: f32 = caps["amount"].parse().ok()?;

            dbg!("get");

            Some(Packet { timestamp, axis, amount })
        } else {
            None
        }
    }
}

pub fn read_packets_file(path: &Path) -> Vec<Packet> {
    let re = Regex::new(r"(?x)
        ^(?P<hours>\d{2}):
        (?P<minutes>\d{2}):
        (?P<seconds>\d{2})\.(?P<millis>\d{3})\s+\|\s+RECEIVE\s+\|\s+
        ENDPOINT\(\[::ffff:\d+\.\d+\.\d+\.\d+\]:\d+\)\s+ADDRESS\(/avatar/parameters/Leash_(?P<axis>[XYZ][+-])\)\s+
        FLOAT\((?P<amount>-?\d+\.\d+)\)
    ").unwrap();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(_) => {
            eprintln!("Error: Unable to open file");
            return Vec::new();
        }
    };

    let mut packets = Vec::new();
    for line in io::BufReader::new(file).lines() {
        if let Ok(line) = line {
            if let Some(packet) = Packet::try_get_from_line(&re, &line) {
                packets.push(packet);
            }
        }
    }

    packets
}

pub struct PlaybackHost {
    global_config: GlobalConfig,
    devices: Vec<DeviceConfig>,
    packets: Vec<Packet>,
    t: f32,

    // timeout: u64,
}

// can this be mutable with all the async shit? lets fkn see lol.

impl PlaybackHost {
    pub fn new() -> Self {
        let (global_config, mut devices) = config::load_config();
        let packets = read_packets_file(Path::new("replays/Lesh.txt"));
        PlaybackHost {
            global_config,
            devices,
            packets,
            t: 0.0,
        }
    }
    pub async fn run(&mut self) {
        let running = Arc::new(AtomicBool::new(false));
        for device in self.devices.iter() {
            let headpat_device_ip_clone = device.device_uri.clone();
            let timeout = self.global_config.timeout;
            task::spawn(async move {
                osc_timeout(&headpat_device_ip_clone, timeout).await.unwrap();
            });
        }

        // Record the starting point
        let start_time = Instant::now();

        for packet in &self.packets {
            // Calculate the delay
            let now = Instant::now();
            let target_time = start_time + packet.timestamp;
            if target_time > now {
                let delay = target_time - now;
                sleep(delay).await;
            }

            // Process the packet
            self.process_packet(packet).await;
        }
        

        


        // maybe a send method
        // or maybe a bespoke for each fkn thing thing, but the thing is like we want to wait a certain duration

    }

    async fn process_packet(&self, packet: &Packet) {
        println!("Processing packet: {:?}", packet);
        for device in self.devices.iter() {
            handle_proximity_parameter::handle_proximity_parameter(
                running.clone(), // Terminator
                value,
                device.clone()
            ).await?
        }

        // rn devices is hacked
        // each device has one motor? or

    }
}

#[async_std::main]
async fn main() {
    let mut host = PlaybackHost::new();
        host.run().await;
}