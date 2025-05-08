/*
    GiggleTech.io - OSC Router
    by Sideways
    Based on OSC Async Library: https://github.com/Frando/async-osc

    This is the main entry point for the GiggleTech OSC router, responsible for receiving and 
    processing Open Sound Control (OSC) messages, managing devices, and adjusting device parameters 
    such as motor speeds and proximity-based triggers.

    **Key Features:**

    1. **Configuration Loading (`config::load_config`)**:
       - Loads global and device-specific configurations from the `config.yml` file.
       - Initializes device parameters such as max speed, proximity settings, and velocity control.
       
    2. **Socket Setup (`giggletech_osc::setup_rx_socket`)**:
       - Sets up the OSC receiver (Rx) socket to listen for incoming OSC messages from devices.
       - Each device's URI and OSC-related settings are configured, allowing the system to communicate properly.

    3. **Timeout Management (`osc_timeout`)**:
       - Each device has a timeout mechanism. If no OSC signal is received within the configured timeout period, 
         the device will stop sending motor control signals.
       - Timeouts are handled concurrently for each device using `task::spawn` to run asynchronously.

    4. **OSC Packet Listening and Processing**:
       - The router listens for OSC packets in a loop, processing each packet as it arrives.
       - Based on the OSC address and data, it:
         - Updates the maximum speed for a device when a max speed parameter is received.
         - Processes proximity signals for headpats, controlling motors or stopping them based on the value received.
       - Utilizes functions from `data_processing` and `handle_proximity_parameter` to adjust motor speeds or handle proximity triggers.

    5. **Motor and Proximity Handling**:
       - When proximity data is received, the system adjusts the motor speed for each device accordingly.
       - If the proximity signal is zero, the device is stopped via the `terminator`.

    **System Tray and Minimization (Future Feature)**:
       - The system tray minimization functionality is planned for future updates, allowing the OSC router to run in the background.

    **Usage**:
    - Run the application to automatically set up device communication and handle proximity/motor controls in real-time.
    - The router listens on the specified OSC ports and adjusts device behavior based on incoming OSC messages.

    **Example Workflow**:
    1. Load configuration for devices.
    2. Set up OSC Rx socket for listening to incoming signals.
    3. Continuously receive and process OSC messages to control devices (e.g., motor speed for headpats).
*/

use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{stream::StreamExt, task::{self}, sync::Arc};
use std::sync::atomic::{AtomicBool};
use std::fs::OpenOptions;
use std::io::{self, Write}; // For file logging and keeping the console open
use chrono::Local; // For getting the local time
use std::path::Path; // Added for checking file existence

use crate::osc_timeout::osc_timeout;
mod data_processing;
mod config;
mod giggletech_osc;
mod terminator;
mod osc_timeout;
mod handle_proximity_parameter;
mod stop_pats;

// Function to log messages to a file with a timestamp
fn log_to_file(message: &str) {
    // Get the current local time
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string(); // Format the time as desired

    // Open the log file in append mode, creating it if it doesn't exist
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("giggletech_log.txt")
        .unwrap();

    // Write the timestamp and the log message to the file
    writeln!(file, "[{}] {}", timestamp, message).unwrap();
}

#[async_std::main]
async fn main() {

    
    // Set a catch-all panic hook to log any panic messages
    std::panic::set_hook(Box::new(|panic_info| {
        let message = format!("Application panicked: {}", panic_info);
        log_to_file(&message);
    }));

    log_to_file("Starting GiggleTech OSC Router...");

    // Call the main logic and handle any errors
    if let Err(e) = run_giggletech().await {
        let error_message = format!("Application encountered an error: {}", e);
        log_to_file(&error_message);
    }

    // Keep the console open even after a crash or an error
    println!("Press Enter to exit...");
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
}

async fn run_giggletech() -> async_osc::Result<()> {
    log_to_file("Loading configuration...");

    // Check if config.yml exists
    if !Path::new("config.yml").exists() {
        log_to_file("Configuration file (config.yml) not found.");
        // Optionally, you might want to return an error here if the config is critical
        // return Err(async_osc::Error::Other("Configuration file not found".into()));
    }

    let (global_config, mut devices) = config::load_config();
    let timeout = global_config.timeout;

    log_to_file("Configuration loaded successfully. Setting up sockets and timeouts.");

    // Setup Start / Stop of Terminator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(global_config.port_rx.to_string()).await?;

    // Timeout management
    for device in devices.iter() {
        let headpat_device_ip_clone = device.device_uri.clone();
        task::spawn(async move {
            if let Err(e) = osc_timeout(&headpat_device_ip_clone, timeout).await {
                let error_message = format!("Timeout error for device {}: {}", headpat_device_ip_clone, e);
                log_to_file(&error_message);
            }
        });
    }

    log_to_file("Listening for OSC Packets...");

    // Listen for OSC Packets
    while let Some(packet) = rx_socket.next().await {
        let (packet, _peer_addr) = packet?;

        // Filter OSC Signals
        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => {
                let (address, osc_value) = message.as_tuple();

                // Handle `/avatar/change` message
                if address == "/avatar/change" {
                    // Check if the first OSC value is a string
                    if let Some(OscType::String(avatar_id)) = osc_value.first() {
                        let log_message = format!("Avatar Changed: {}", avatar_id);
                        log_to_file(&log_message);
                    }
                    continue; // Skip the rest of the loop as this is handled
                }

                // Handle other messages
                let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float() {
                    Some(v) => v,
                    None => continue,
                };

                for device in devices.iter_mut() {
                    // Max Speed Setting
                    if address == *device.max_speed_parameter {
                        data_processing::print_speed_limit(value);
                        device.max_speed = value.max(global_config.minimum_max_speed);
                        //let log_message = format!("Updated max speed for device: {} to {}", device.device_uri, device.max_speed);
                        //log_to_file(&log_message);
                    } else if address == *device.proximity_parameter {
                        handle_proximity_parameter::handle_proximity_parameter(
                            running.clone(), // Terminator
                            value,
                            device.clone()
                        ).await?;
                        //let log_message = format!("Processed proximity parameter for device: {}", device.device_uri);
                        //log_to_file(&log_message);
                    }
                }
            }
        }
    }

    Ok(())
}
