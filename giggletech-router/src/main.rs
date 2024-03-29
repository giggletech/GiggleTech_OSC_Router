// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc


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
    ) = config::load_config();

    // Setup Start / Stop of Terminiator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(port_rx).await?;

    // Timeout
    for ip in &headpat_device_uris {
        let headpat_device_ip_clone = ip.clone();
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

                // Max Speed Setting
                if address == max_speed_parameter_address {
                    data_processing::print_speed_limit(value);
                    max_speed = value.max(max_speed_low_limit);
                } else {
                    let index = proximity_parameters_multi.iter().position(|a| *a == address);

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
