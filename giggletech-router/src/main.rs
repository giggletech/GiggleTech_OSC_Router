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

    use std::io::{stdin, stdout, Write};
    use std::thread;
    use std::time::Duration;
    
    let steps = 20; // set the number of steps here
    
    loop {
        let mut input = String::new();
    
        for osc_address in &proximity_parameters_multi {
            for i in 0..2 {
                for j in 0..=steps {
                    let value = if i == 0 {
                        j as f32 / steps as f32
                    } else {
                        (steps - j) as f32 / steps as f32
                    };
                    println!("sending value: {} to osc_address: {}", value, osc_address);
                    giggletech_osc::send_data("127.0.0.1", osc_address, &port_rx, value).await?;
                    thread::sleep(Duration::from_millis(100));
                }
            }
            println!("Press enter to test next device");
            stdout().flush()?;
            stdin().read_line(&mut input)?;
            if input.trim() == "" {
                input.clear();
            } else {
                break;
            }
        }
    
        println!("All devices have been tested. Press enter to test again or type 'quit' to exit");
        stdout().flush()?;
        input.clear();
        stdin().read_line(&mut input)?;
    
        if input.trim() == "quit" {
            break;
        }
    }
    
    





    Ok(())
}
