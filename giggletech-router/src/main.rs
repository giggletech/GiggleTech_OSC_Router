// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc


use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{stream::StreamExt, task::{self}, sync::Arc,};
use std::{time::{Instant}};
use std::sync::atomic::{AtomicBool};

use crate::osc_timeout::osc_timeout;
mod data_processing;
mod config;
mod giggletech_osc;
mod terminator;
mod osc_timeout;






async fn handle_proximity_parameter(
    running: Arc<AtomicBool>,
    device_ip: &Arc<String>,
    value: f32,
    max_speed: f32,
    min_speed: f32,
    speed_scale: f32,
    proximity_parameter_address: &str,
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    // Update Last Signal Time for timeout clock
        
    let mut last_signal_time = osc_timeout::LAST_SIGNAL_TIME.lock().unwrap();
    *last_signal_time = Instant::now();
    if value == 0.0 {
        println!("Stopping pats...");
        terminator::start(running.clone(), &device_ip).await?;

        for _ in 0..5 {
            giggletech_osc::send_data(&device_ip, 0i32).await?;  
        }
    } else {
        //start_timer(value);
        giggletech_osc::send_data(&device_ip,
            data_processing::process_pat(value, max_speed, min_speed, speed_scale)).await?;
    }
    Ok(())
}




#[async_std::main]
async fn main() -> Result<()> {
    // Import Config
    // Todo: Refactor
    let (
        headpat_device_ip,
        headpat_device_uris,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameter_addresses,
        proximity_parameters_multi,
        max_speed_parameter_address,
        max_speed_low_limit,
    ) = config::load_config();

    // Setup Start / Stop of Terminiator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(port_rx).await?;

    // Timeout
    let headpat_device_ip_clone = headpat_device_ip.clone();
    task::spawn(async move {
        osc_timeout(&headpat_device_ip_clone).await.unwrap();
    });

    // Listen for OSC Packets
    while let Some(packet) = rx_socket.next().await {
        let (packet, _peer_addr) = packet?;

        // Filter OSC Signals: Headpat Max & Headpat Prox
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
                            handle_proximity_parameter(
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
