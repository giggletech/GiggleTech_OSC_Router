// handle_proximity_parameter.rs

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
    device: DeviceConfig,
) -> Result<()> {
    println!("handle proximity parameter {}", device.proximity_parameter);
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
            giggletech_osc::send_data(&device_ip, &device.motor_address, 0i32).await?;  
        }
    } else {
        if !device.use_velocity_control {
            println!("no v control");
            giggletech_osc::send_data(&device_ip,   &device.motor_address,
                data_processing::process_pat(value, &device)).await?;
        } else {
            let delta_t = match last_signal_time {
                None => Duration::new(0, 0),
                Some(t_prev) => Instant::now().duration_since(t_prev),
            };

            giggletech_osc::send_data(&device_ip, &device.motor_address,
                data_processing::process_pat_advanced(value, last_val, delta_t, &device)).await?;
        }


    }
    Ok(())
}
