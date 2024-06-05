use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use std::time::{Instant, Duration};
use regex::Regex;
use std::sync::atomic::AtomicBool;
use async_std::task;
use async_std::sync::Arc;
use async_std::task::sleep;
use std::collections::HashMap;

mod config;
use config::*;
mod osc_timeout;
use osc_timeout::*;

mod data_processing;
mod giggletech_osc;
mod terminator;
mod handle_proximity_parameter;

// todo remove or change the other thing to be this
pub struct ParseContext {
    re_line: Regex,
    re_timestamp: Regex,
    re_float: Regex,
    re_bool: Regex,
}

impl ParseContext {
    pub fn new() -> Self {
        ParseContext {
            re_line: Regex::new(r"^([^|]+) \| RECEIVE\s+\| ENDPOINT\(\[[^\]]+\]:\d+\) ADDRESS\(/avatar/parameters/Leash_([^)]+)\) (.+)$")
                .unwrap(),
            re_timestamp: Regex::new(r"(\d+):(\d+):(\d+)\.(\d+)").unwrap(),
            re_float: Regex::new(r"FLOAT\(([-+]?[0-9]*\.?[0-9]+)\)").unwrap(),
            re_bool: Regex::new(r"BOOL\((TRUE|FALSE)\)").unwrap(),
        }
    }

    pub fn decode_line(&self, line: &str) -> Option<InPacket> {
        let caps = self.re_line.captures(line)?;
        let timestamp = caps.get(1)?.as_str().to_owned();
        let param = caps.get(2)?.as_str().to_owned();
        let arg = caps.get(3)?.as_str().to_owned();
        Some(InPacket { 
            timestamp,
            param, 
            arg
        })
    }

    pub fn decode_packet(&self, p: InPacket) -> Option<Packet> {
        // Parse the timestamp
        let caps = self.re_timestamp.captures(&p.timestamp)?;
        let hours: u64 = caps.get(1)?.as_str().parse().ok()?;
        let minutes: u64 = caps.get(2)?.as_str().parse().ok()?;
        let seconds: u64 = caps.get(3)?.as_str().parse().ok()?;
        let millis: u64 = caps.get(4)?.as_str().parse().ok()?;
        let timestamp = Duration::from_secs(hours * 3600 + minutes * 60 + seconds) + Duration::from_millis(millis);

        // Parse the argument
        let arg = if let Some(caps) = self.re_float.captures(&p.arg) {
            caps.get(1)?.as_str().parse().ok()?
        } else if let Some(caps) = self.re_bool.captures(&p.arg) {
            match caps.get(1)?.as_str() {
                "TRUE" => 1.0,
                "FALSE" => 0.0,
                _ => return None,
            }
        } else {
            return None;
        };

        Some(Packet {
            timestamp,
            param: p.param,
            arg,
        })
    }
}

// todo remove or change the other thing to be this
#[test]
fn test_decode_line() {
    let s1 = "19:59:50.408 | RECEIVE    | ENDPOINT([::ffff:127.0.0.1]:54657) ADDRESS(/avatar/parameters/Leash_Angle) FLOAT(0.3603651)";
    let s2 = "20:00:37.377 | RECEIVE    | ENDPOINT([::ffff:127.0.0.1]:54657) ADDRESS(/avatar/parameters/Leash_IsGrabbed) BOOL(FALSE)";
    let r1 = InPacket {
        timestamp: "19:59:50.408".to_owned(),
        param: "Angle".to_owned(),
        arg: "FLOAT(0.3603651)".to_owned(),
    };
    let r2 = InPacket {
        timestamp: "20:00:37.377".to_owned(),
        param: "IsGrabbed".to_owned(),
        arg: "BOOL(FALSE)".to_owned(),
    };
    let pc = ParseContext::new();
    assert_eq!(pc.decode_line(s1), Some(r1));
    assert_eq!(pc.decode_line(s2), Some(r2));
}

#[test]
fn test_decode_packet() {
    let pc = ParseContext::new();
    let ip1 = InPacket {
        timestamp: "19:59:50.408".to_owned(),
        param: "Angle".to_owned(),
        arg: "FLOAT(0.3603651)".to_owned(),
    };
    let ip2 = InPacket {
        timestamp: "20:00:37.377".to_owned(),
        param: "IsGrabbed".to_owned(),
        arg: "BOOL(FALSE)".to_owned(),
    };
    let p1 = Packet {
        timestamp: Duration::new(71990, 408_000_000),
        param: "Angle".to_owned(),
        arg: 0.3603651,
    };
    let p2 = Packet {
        timestamp: Duration::new(72037, 377_000_000),
        param: "IsGrabbed".to_owned(),
        arg: 0.0,
    };
    assert_eq!(pc.decode_packet(ip1), Some(p1));
    assert_eq!(pc.decode_packet(ip2), Some(p2));
}

// todo remove or change the other thing to be this
#[derive(PartialEq, Debug)]
pub struct InPacket {
    timestamp: String,
    param: String,
    arg: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Packet {
    timestamp: Duration,
    param: String,  // x+ x- y+ y- z+ z- Angle Stretch IsGrabbed
    arg: f32,
}

impl Packet {
    pub fn try_get_from_line(re: &Regex, line: &str) -> Option<Packet> {
        if let Some(caps) = re.captures(line) {
            let hours: u64 = caps["hours"].parse().ok()?;
            let minutes: u64 = caps["minutes"].parse().ok()?;
            let seconds: u64 = caps["seconds"].parse().ok()?;
            let millis: u64 = caps["millis"].parse().ok()?;

            let timestamp = Duration::from_secs(hours * 3600 + minutes * 60 + seconds) + Duration::from_millis(millis);
            let param = caps["param"].to_string();
            
            let arg = match param.as_str() {
                "IsGrabbed" => {
                    if &caps["amountb"] == "TRUE" {
                        1.0
                    } else {
                        0.0
                    }
                },
                _ => caps["amount"].parse().ok()?
            };

            Some(Packet { timestamp, param, arg })
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
        ENDPOINT\(\[::ffff:\d+\.\d+\.\d+\.\d+\]:\d+\)\s+ADDRESS\(/avatar/parameters/Leash_(?P<param>[XYZ][+-]|IsGrabbed|Stretch)\)\s+
        (FLOAT\((?P<amount>-?\d+\.\d+)\)|BOOL\((?P<amountb>(TRUE|FALSE))\))
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

pub struct PlaybackState {
    state: HashMap<String, f32>,
}

impl Default for PlaybackState {
    fn default() -> Self {
        let mut m = HashMap::new();
        m.insert("X+".to_owned(), 0.0f32);
        m.insert("Y+".to_owned(), 0.0);
        m.insert("Z+".to_owned(), 0.0);
        m.insert("X-".to_owned(), 0.0);
        m.insert("Y-".to_owned(), 0.0);
        m.insert("Z-".to_owned(), 0.0);
        m.insert("Angle".to_owned(), 0.0);
        m.insert("Stretch".to_owned(), 0.0);
        m.insert("IsGrabbed".to_owned(), 0.0); // nb this a bool im just gonna make 0.0 or 1.0
        PlaybackState {
            state: m,
        }
    }
}

impl PlaybackState {
    pub fn take_packet(&mut self, p: &Packet) {
        if let Some(v) = self.state.get_mut(&p.param) {
            *v = p.arg;
        }
    }
    // uh deadzone or anything?
    pub fn get_current(&self) -> (f32, f32) {
        //let r = self.state["IsGrabbed"]*self.state["Stretch"];
        // let r = 10.0*self.state["IsGrabbed"]*self.state["Stretch"];
        let r = 1.0;
        let u = self.state["X+"] - self.state["X-"];
        let v = self.state["Z+"] - self.state["Z-"];
        (r*u, r*v)
    }
}

pub struct PlaybackHost {
    global_config: GlobalConfig,
    devices: Vec<DeviceConfig>,
    packets: Vec<Packet>,
    running: Arc<AtomicBool>,
    state: PlaybackState
}

impl PlaybackHost {
    pub fn new() -> Self {
        let (global_config, devices) = config::load_config();
        let packets = read_packets_file(Path::new("giggletech-router/replays/Lesh.txt"));
        // let packets = read_packets_file(Path::new("giggletech-router/replays/test.txt")); // strraight up doesnt wsork idk
        PlaybackHost {
            global_config,
            devices,
            packets,
            running: Arc::new(AtomicBool::new(false)),
            state: PlaybackState::default(),
        }
    }
    pub async fn run(&mut self) {
        for device in self.devices.iter() {
            let headpat_device_ip_clone = device.device_uri.clone();
            let timeout = self.global_config.timeout;
            task::spawn(async move {
                osc_timeout(&headpat_device_ip_clone, timeout).await.unwrap();
            });
        }

        let start_time = Instant::now();
        let packets_start = self.packets[0].timestamp;

        for i in 0..self.packets.len() {
            let packet = self.packets[i].clone();
            let now = Instant::now();
            let t_since_start = now - start_time;
            let packet_t_since_start = packet.timestamp - packets_start;

            if t_since_start >= packet_t_since_start {
                self.process_packet(&packet).await;
            } else {
                let delta = packet_t_since_start - t_since_start;
                sleep(delta).await;
            }
        }
    }

    async fn process_packet(&mut self, packet: &Packet) {
        self.state.take_packet(packet);
        let (u, v) = self.state.get_current();
        let xp = u.max(0.0);
        let xm = -(u.min(0.0));
        let zp = v.max(0.0);
        let zm = -(v.min(0.0));

        let dxp = self.devices.iter().find(|d| {
            *d.proximity_parameter == "/avatar/parameters/X+".to_owned()
        }).unwrap();
        let dxm = self.devices.iter().find(|d| {
            *d.proximity_parameter == "/avatar/parameters/X-".to_owned()
        }).unwrap();
        let dzp = self.devices.iter().find(|d| {
            *d.proximity_parameter == "/avatar/parameters/Z+".to_owned()
        }).unwrap();
        let dzm = self.devices.iter().find(|d| {
            *d.proximity_parameter == "/avatar/parameters/Z-".to_owned()
        }).unwrap();

        handle_proximity_parameter::handle_proximity_parameter(
            self.running.clone(),
            xp,
            dxp.clone(),
        ).await.unwrap();

        handle_proximity_parameter::handle_proximity_parameter(
            self.running.clone(),
            xm,
            dxm.clone(),
        ).await.unwrap();

        handle_proximity_parameter::handle_proximity_parameter(
            self.running.clone(),
            zp,
            dzp.clone(),
        ).await.unwrap();
        handle_proximity_parameter::handle_proximity_parameter(
            self.running.clone(), // Terminator
            zm,
            dzm.clone(),
        ).await.unwrap();
    }
}

#[async_std::main]
async fn main() {
    let mut host = PlaybackHost::new();
        host.run().await;
        println!("finishing up...");
        sleep(Duration::from_secs(host.global_config.timeout + 1)).await;
        println!("done");
}


// maybe any prox of 0 runs terminator