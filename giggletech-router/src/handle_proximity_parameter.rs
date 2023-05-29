// handle_proximity_parameter.rs

use async_osc::Result;
use async_std::sync::Arc;
use std::{
    sync::atomic::{AtomicBool},
    time::Instant,
};

use once_cell::sync::Lazy;
use std::collections::VecDeque;

use crate::osc_timeout;
use crate::terminator;
use crate::giggletech_osc;
use crate::data_processing;
use crate::kays_pat_decay;

use std::sync::{Mutex};

pub(crate) async fn handle_proximity_parameter(
    running: Arc<AtomicBool>,
    device_ip: &Arc<String>,
    value: f32,
    max_speed: f32,
    min_speed: f32,
    speed_scale: f32,
    proximity_parameters_multi: &String,
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    // Update Last Signal Time for timeout clock 
    let mut device_last_signal_times = osc_timeout::DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
    device_last_signal_times.insert(device_ip.to_string(), Instant::now());

    let mut device_last_signal_times = kays_pat_decay::DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
    device_last_signal_times.insert(device_ip.to_string(), Instant::now());

    // Add a Mutex to hold the last value
    //static LAST_VALUES: Lazy<Mutex<Option<f32>>> = Lazy::new(|| Mutex::new(None));'
    static LAST_VALUES: Lazy<Mutex<VecDeque<f32>>> = Lazy::new(|| Mutex::new(VecDeque::with_capacity(5)));
    
    if value == 0.0 {
        println!("Stopping pats...");
        terminator::start(running.clone(), &device_ip).await?;

        for _ in 0..5 {
            giggletech_osc::send_data(&device_ip, 0i32).await?;  
        }
    } else {
        // Print the last values
        if let Ok(last_values_guard) = LAST_VALUES.lock() {
            println!("Last values: {:?}", *last_values_guard);
        }
        
        // Update the last values
        if let Ok(mut last_values_guard) = LAST_VALUES.lock() {
            last_values_guard.push_front(value);
            
            if last_values_guard.len() > 5 {
                last_values_guard.pop_back();
            }
        }
        
        // Send Data
        giggletech_osc::send_data(&device_ip,
            data_processing::process_pat(value, max_speed, min_speed, speed_scale, proximity_parameters_multi)).await?;

    }
    Ok(())
}
