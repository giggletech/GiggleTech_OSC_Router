// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc

// Add System Tray Minimization


use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{stream::StreamExt, task::{self}, sync::Arc,};
use std::sync::atomic::{AtomicBool};

use crate::osc_timeout::osc_timeout;
mod data_processing;
mod config;
mod giggletech_osc;
mod terminator;
mod osc_timeout;
mod handle_proximity_parameter;


#[async_std::main]
async fn main() -> Result<()> {
    let (global_config, mut devices) = config::load_config();
    let timeout = global_config.timeout;

    // Setup Start / Stop of Terminiator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(global_config.port_rx.clone()).await?;

    // Timeout
    for device in devices.iter() {
        let headpat_device_ip_clone = device.device_uri.clone();
        task::spawn(async move {
            osc_timeout(&headpat_device_ip_clone, timeout).await.unwrap();
        });
    }
    // Listen for OSC Packets
    while let Some(packet) = rx_socket.next().await {
        let (packet, _peer_addr) = packet?;

        // Filter OSC Signals
        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => {
                let (address, osc_value) = message.as_tuple();
                let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float() {
                    Some(v) => v,
                    None => continue,
                };

                for device in devices.iter_mut() {
                    // Max Speed Setting
                    if address == device.max_speed_parameter {
                        data_processing::print_speed_limit(value);
                        device.max_speed = value.max(global_config.minimum_max_speed);
                    } else if address == device.proximity_parameter {
                        handle_proximity_parameter::handle_proximity_parameter(
                            running.clone(), // Terminator
                            value,
                            device.clone()
                        ).await?
                    }
                }
            }
        }
    }
    Ok(())
}
