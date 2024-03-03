// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc

#![windows_subsystem = "windows"]

use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{
    stream::StreamExt,
    sync::Arc,
    task::{self},
};
use log::info;
use std::sync::atomic::AtomicBool;

use crate::osc_timeout::osc_timeout;
mod config;
mod data_processing;
mod giggletech_osc;
mod handle_proximity_parameter;
mod osc_timeout;
mod terminator;
mod tray; 
mod logger;

#[async_std::main]
async fn main() -> Result<()> {
    if let Err(e) = logger::init_logging() {
        println!("Error initializing logging: {}", e);
        return Ok(());
    }

    info!("This is an information message.");

    _ = task::spawn(handle_osc());

    tray::setup_and_run_tray();
    
    Ok(())
}

async fn handle_osc() -> Result<()> {
    let (
        headpat_device_uris,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameters_multi,
        max_speed_parameter_address,
        max_speed_low_limit,
        timeout,
        advanced_config,
    ) = config::load_config();

    // Setup Start / Stop of Terminiator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(port_rx).await?;

    // Timeout
    for ip in &headpat_device_uris {
        let headpat_device_ip_clone = ip.clone();
        task::spawn(async move {
            osc_timeout(&headpat_device_ip_clone, timeout)
                .await
                .unwrap();
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

                // Max Speed Setting
                if address == max_speed_parameter_address {
                    data_processing::print_speed_limit(value);
                    max_speed = value.max(max_speed_low_limit);
                } else {
                    let index = proximity_parameters_multi
                        .iter()
                        .position(|a| *a == address);

                    match index {
                        Some(i) => {
                            handle_proximity_parameter::handle_proximity_parameter(
                                running.clone(), // Terminator
                                &Arc::new(headpat_device_uris[i].clone()),
                                value,
                                max_speed,
                                min_speed,
                                speed_scale,
                                &proximity_parameters_multi[i],
                                advanced_config,
                            )
                            .await?
                        }
                        None => {}
                    }
                }
            }
        }
    }
    Ok(())
}

