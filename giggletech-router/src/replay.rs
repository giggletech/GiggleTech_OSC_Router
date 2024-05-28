use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::time::{Instant, Duration};
use regex::Regex;

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

pub fn read_file(path: &Path) -> Vec<Packet> {
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

    }

    pub fn play(&mut self) -> PlaybackState {

    }
}

pub struct PlaybackState {
    pub t: f32,
}

impl PlaybackState {
    pub fn tick(&mut self, dt: f32, host: &PlaybackHost) {

    }
}

fn main() {
    let v = read_file(Path::new("giggletech-router/replays/Lesh.txt"));
    dbg!(v);
}