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
use async_std::{stream::StreamExt, task::{self}, sync::Arc,};
use std::sync::atomic::{AtomicBool};

use crate::osc_timeout::osc_timeout;
mod data_processing;
mod config;
mod giggletech_osc;
mod terminator;
mod osc_timeout;
mod handle_proximity_parameter;
mod stop_pats;



#[async_std::main]
async fn main() -> Result<()> {
    let (global_config, mut devices) = config::load_config();
    let timeout = global_config.timeout;

    // Setup Start / Stop of Terminator
    let running = Arc::new(AtomicBool::new(false));

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(global_config.port_rx.to_string()).await?;

    // Timeout
    for device in devices.iter() {
        let headpat_device_ip_clone = device.device_uri.clone();
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

                // Handle `/avatar/change` message
                if address == "/avatar/change" {
                    // Check if the first OSC value is a string
                    if let Some(OscType::String(avatar_id)) = osc_value.first() {
                        // Print "Avatar Changed" along with the avatar ID
                        println!("Avatar Changed: {}", avatar_id);
                        //stop_pats::stop_pats(&device).await?;  // Pass the device config
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
                    } else if address == *device.proximity_parameter {
                        handle_proximity_parameter::handle_proximity_parameter(
                            running.clone(), // Terminator
                            value,
                            device.clone()
                        ).await?;
                    }
                }
            }
        }
    }



    
    Ok(())
}
