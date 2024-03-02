// handle_proximity_parameter.rs

use async_osc::Result;
use async_std::sync::{Arc, Mutex};
use std::{
    sync::atomic::{AtomicBool},
    time::{Instant, Duration}, collections::HashMap,
};


use crate::{osc_timeout, config::AdvancedConfig};
use crate::terminator;
use crate::giggletech_osc;
use crate::data_processing;
use lazy_static::lazy_static;


lazy_static! {
    pub static ref DEVICE_LAST_VALUE: Arc<Mutex<HashMap<String, f32>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub(crate) async fn handle_proximity_parameter(
    running: Arc<AtomicBool>,
    device_ip: &Arc<String>,
    value: f32,
    max_speed: f32,
    min_speed: f32,
    speed_scale: f32,
    proximity_parameters_multi: &String,
    advanced_config: AdvancedConfig,
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    // Update Last Signal Time for timeout clock 
    let mut device_last_signal_times = osc_timeout::DEVICE_LAST_SIGNAL_TIME.lock().await;
    // let last_signal_time: Option<Instant> = device_last_signal_times.get(&device_ip.to_string()).copied();
    let last_signal_time = device_last_signal_times.insert(device_ip.to_string(), Instant::now());
    let mut device_last_values = DEVICE_LAST_VALUE.lock().await;
    let last_val = match device_last_values.insert(device_ip.to_string(), value) {
        None => 0.0,
        Some(v) => v,
    };

    if value == 0.0 {
        println!("Stopping pats...");
        terminator::start(running.clone(), &device_ip).await?;

        for _ in 0..5 {
            giggletech_osc::send_data(&device_ip, 0i32).await?;  
        }
    } else {
        if !advanced_config.active {
            giggletech_osc::send_data(&device_ip,
                data_processing::process_pat(value, max_speed, min_speed, speed_scale, proximity_parameters_multi)).await?;
        } else {
            let delta_t = match last_signal_time {
                None => Duration::new(0, 0),
                Some(t_prev) => Instant::now().duration_since(t_prev),
            };

            giggletech_osc::send_data(&device_ip,
                data_processing::process_pat_advanced(value, last_val, delta_t, max_speed, min_speed, speed_scale, proximity_parameters_multi, advanced_config)).await?;
        }


    }
    Ok(())
}
