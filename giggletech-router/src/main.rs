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


    for osc_address in &proximity_parameters_multi {
        
        println!("osc_address: {}",  osc_address);
        giggletech_osc::send_data("127.0.0.1",  osc_address, &port_rx, 0.9).await?;
        thread::sleep(Duration::from_secs(1));
        

    }
    





    Ok(())
}
