/*
    handle_proximity_parameter.rs - Handling Proximity Data for GiggleTech Devices

    This module processes proximity sensor data and controls device actions (like motors) based on 
    the proximity values. It tracks the last proximity signal for each device and manages sending 
    commands to the device via OSC.

    **Key Features:**

    1. **Proximity Handling (`handle_proximity_parameter`)**:
       - Receives proximity data (`value`) and determines if the device should stop or continue operating.
       - If proximity is zero, it sends stop commands to the device.
       - If proximity is non-zero, it processes the proximity data and sends motor control values to the device.

    2. **Velocity Control**:
       - If the device uses velocity control, the module calculates the change in proximity over time and adjusts the motor speed accordingly.
       - Otherwise, it simply scales the motor value based on proximity.

    3. **Timeout and Signal Tracking**:
       - Updates the last signal time and last proximity value for each device, ensuring proper handling of timeouts and avoiding stale data.

    **Usage**:
    - This function is typically called when proximity data is received and determines the appropriate action (start, stop, or adjust motor) for the device.
*/

use async_osc::Result;
use async_std::sync::{Arc, Mutex};
use std::{
    sync::atomic::{AtomicBool},
    time::{Instant, Duration}, collections::HashMap,
};


use crate::osc_timeout;
use crate::terminator;
use crate::giggletech_osc;
use crate::data_processing;
use lazy_static::lazy_static;
use crate::config::DeviceConfig;


lazy_static! {
    pub static ref DEVICE_LAST_VALUE: Arc<Mutex<HashMap<String, f32>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub(crate) async fn handle_proximity_parameter(
    running: Arc<AtomicBool>,
    value: f32,
    device: DeviceConfig
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    let device_ip = Arc::new(device.device_uri.clone());

    // Update Last Signal Time for timeout clock 
    let mut device_last_signal_times = osc_timeout::DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
    // let last_signal_time: Option<Instant> = device_last_signal_times.get(&device_ip.to_string()).copied();
    let last_signal_time = device_last_signal_times.insert(device_ip.to_string(), Instant::now());
    let mut device_last_values = DEVICE_LAST_VALUE.lock().await;
    let last_val = device_last_values.insert(device_ip.to_string(), value).unwrap_or(0.0);

    if value == 0.0 {
        println!("Stopping pats...");
        terminator::start(running.clone(), &device_ip).await?;

        for _ in 0..5 {
            giggletech_osc::send_data(&device_ip, 0i32).await?;  
        }
    } else {
        if !device.use_velocity_control {
            giggletech_osc::send_data(&device_ip,
                data_processing::process_pat(value, &device, last_val)).await?;
        } else {
            let delta_t = match last_signal_time {
                None => Duration::new(0, 0),
                Some(t_prev) => Instant::now().duration_since(t_prev),
            };

            giggletech_osc::send_data(&device_ip,
                data_processing::process_pat_advanced(value, last_val, delta_t, &device)).await?;
        }
    }
    Ok(())
}
