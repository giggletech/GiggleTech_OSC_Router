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

use async_osc::{prelude::*, OscPacket, OscType, Result};
use futures::StreamExt;
use crate::osc_timeout::osc_timeout;

mod config;
use config::*;
mod osc_timeout;
use osc_timeout::*;

mod data_processing;
mod giggletech_osc;
mod terminator;
mod handle_proximity_parameter;

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
    pub fn take_packet(&mut self, param: String, arg: f32) {
        println!("take {} {}", param, arg);
        if let Some(v) = self.state.get_mut(&param) {
            *v = arg;
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

pub struct Router {
    global_config: GlobalConfig,
    devices: Vec<DeviceConfig>,
    running: Arc<AtomicBool>,
    state: PlaybackState
}

impl Router {
    pub fn new() -> Self {
        let (global_config, devices) = config::load_config();
        Router {
            global_config,
            devices,
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
        // Rx/Tx Socket Setup
        let mut rx_socket = giggletech_osc::setup_rx_socket(&self.global_config.port_rx).await.expect("no rx socket");


        // Listen for OSC Packets
        while let Some(packet) = rx_socket.next().await {
            let (packet, _peer_addr) = packet.unwrap();

            // Filter OSC Signals
            match packet {
                OscPacket::Bundle(_) => {}
                OscPacket::Message(message) => {
                    let (address, osc_value) = message.as_tuple();
                    let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float() {
                        Some(v) => v,
                        None => continue,
                    };
                    let address = address.split("/").last().unwrap();
                    // dunno if we r taking the bool parameter or nah (also want to ignoe it or nah)
                    self.process_packet(address.to_owned(), value).await;
                }
            }
        }
    }

    async fn process_packet(&mut self, param: String, arg: f32) {
        self.state.take_packet(param, arg);
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
    let mut host = Router::new();
        host.run().await;
        println!("finishing up...");
        sleep(Duration::from_secs(host.global_config.timeout + 1)).await;
        println!("done");
}


// maybe any prox of 0 runs terminator