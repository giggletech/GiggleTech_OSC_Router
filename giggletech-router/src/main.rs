// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc


use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{stream::StreamExt, task::{self}, sync::Arc,};
use std::sync::atomic::{AtomicBool};
use std::time::Duration;
use std::thread;

use std::io::stdin;
use std::net::IpAddr;








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
        //let headpat_device_ip_clone = ip.clone();
        println!("ip {}", ip);
        giggletech_osc::send_data(ip, 100).await?;
        thread::sleep(Duration::from_secs(1));
        giggletech_osc::send_data(ip, 0).await?;

    }
    





    Ok(())
}
