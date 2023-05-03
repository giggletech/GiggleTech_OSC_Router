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

    use std::thread;
    use std::time::Duration;
    
    let steps = 10; // set the number of steps here
    
    for osc_address in &proximity_parameters_multi {
        for i in 0..2 {
            for j in 0..=steps {
                let value = if i == 0 {
                    j as f32 / steps as f32
                } else {
                    (steps - j) as f32 / steps as f32
                };
                println!(" {}: value: {}", osc_address, value);
                giggletech_osc::send_data("127.0.0.1", osc_address, &port_rx, value).await?;
                thread::sleep(Duration::from_millis(100));
            }
        }
    }
    
    





    Ok(())
}
