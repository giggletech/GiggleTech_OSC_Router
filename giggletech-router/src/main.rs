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

#[async_std::main]
async fn main() -> Result<()> {
     
    // Import Config 
    // Todo: Refactor
    let (headpat_device_ip,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        max_speed_low_limit,

    ) = config::load_config();

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(port_rx).await?;

 
    // Timeout
    let headpat_device_ip_clone = headpat_device_ip.clone(); 
    task::spawn(async move {
        osc_timeout(&headpat_device_ip_clone).await.unwrap();
    });

    // Start/ Stop Function Setup
    let running = Arc::new(AtomicBool::new(false));
    let headpat_device_ip_arc = Arc::new(headpat_device_ip);

    // Listen for OSC Packets
    while let Some(packet) = rx_socket.next().await {
        let (packet, _peer_addr) = packet?;
        
        // Filter OSC Signals : Headpat Max & Headpat Prox 
        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => {

                let (address, osc_value) = message.as_tuple();
                let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float(){
                    Some(v) => v, 
                    None => continue,
                };

                // Max Speed Setting
                if address == max_speed_parameter_address {
                    data_processing::print_speed_limit(value);
                    max_speed = value.max(max_speed_low_limit);
                }
                
                // Prox Parmeter 
                else if address == proximity_parameter_address  {
                    
                    terminator::stop(running.clone()).await?;
                    // Update Last Signal Time for timeout clock
                    let mut last_signal_time = osc_timeout::LAST_SIGNAL_TIME.lock().unwrap();
                    *last_signal_time = Instant::now();

                    // Stop Function
                    if value == 0.0 {
                        println!("Stopping pats...");
                        terminator::start(running.clone(), &headpat_device_ip_arc).await?;

                        for _ in 0..5 {
                            giggletech_osc::send_data(&headpat_device_ip_arc, 0i32).await?;  
                        }

                    } else {
                        giggletech_osc::send_data(&headpat_device_ip_arc,
                            data_processing::process_pat(value, max_speed, min_speed, speed_scale)).await?;
                    }
                }
                else {
                    //eprintln!("Unknown Address") // Have a debug mode, print if debug mode
                }
            } 
        }  
    }
    Ok(())
}