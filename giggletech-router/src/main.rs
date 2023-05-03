// GiggleTech.io
// OSC Router
// by Sideways
// Based off OSC Async https://github.com/Frando/async-osc


use async_osc::{Result};
use std::time::Duration;
use std::thread;



mod config;
mod giggletech_osc;

use std::io::{stdin, stdout, Write};

fn wait_for_enter() {
    let mut input = String::new();
    println!("Press 'Enter' to continue...");
    stdout().flush().expect("Failed to flush stdout");
    stdin().read_line(&mut input).expect("Failed to read line from stdin");
}


async fn cycle_values(steps: i32, osc_address: &str, port_rx: &str) -> Result<()> {
    for i in 0..2 {
        for j in 0..=steps {
            let value = if i == 0 {
                j as f32 / steps as f32
            } else {
                (steps - j) as f32 / steps as f32
            };
            println!("{}: {}", osc_address, value);
            giggletech_osc::send_data("127.0.0.1", osc_address, port_rx, value).await?;
            thread::sleep(Duration::from_millis(50));
        }
    }
    Ok(())
}


async fn cycle_max_speed_up(steps: i32, _osc_address: &str, port_rx: &str) -> Result<()> {
    for j in 0..=steps {
        let value = j as f32 / steps as f32;
        println!("/avatar/parameters/max_speed: {}", value);
        giggletech_osc::send_data("127.0.0.1", "/avatar/parameters/max_speed", &port_rx, value).await?;
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}



#[async_std::main]
async fn main() -> Result<()> {

    let (
        _headpat_device_uris,
        _min_speed,
        mut _max_speed,
        _speed_scale,
        port_rx,
        proximity_parameters_multi,
        _max_speed_parameter_address,
        _max_speed_low_limit,
        _timeout,
    ) = config::load_config();

    use std::io::{stdin, stdout, Write};

    

    
    let steps = 10; // set the number of steps here
    
    loop {
        let mut input = String::new();
    
        for osc_address in &proximity_parameters_multi {
            
            println!("Cycle {} at 20% Speed", osc_address);
            println!("");
            giggletech_osc::send_data("127.0.0.1", "/avatar/parameters/max_speed", &port_rx, 0.2).await?;

            wait_for_enter();

            cycle_values(steps, osc_address, &port_rx).await?;
            cycle_values(steps, osc_address, &port_rx).await?;
            cycle_values(steps, osc_address, &port_rx).await?;

            println!("\nCycle complete \n");
            println!("Set Device to Max Speed \n");
            wait_for_enter();

            cycle_max_speed_up(steps, osc_address, &port_rx).await?;
            println!("\nTest Device at Max Speed \n");
            wait_for_enter();

            cycle_values(steps, osc_address, &port_rx).await?;
            cycle_values(steps, osc_address, &port_rx).await?;
            cycle_values(steps, osc_address, &port_rx).await?;

            println!("\nReset max speed to 20%");
            giggletech_osc::send_data("127.0.0.1", "/avatar/parameters/max_speed", &port_rx, 0.2).await?;
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
