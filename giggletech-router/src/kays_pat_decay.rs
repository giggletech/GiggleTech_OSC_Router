// osc_timeout.rs

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use anyhow::Result;
use lazy_static::lazy_static;
use crate::giggletech_osc;

lazy_static! {
    pub static ref DEVICE_LAST_SIGNAL_TIME: Arc<Mutex<HashMap<String, Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

// The time is updated in handle_proximity_paramter module


/*
What do i want it to do?

Ramp down after prox signal has ramped up, even if prox signal has platued

Ignore is delta -> +
Start ramp down if no change

If new value is not > old by X amount start ramp down to 0

must reset to 0 before going agagin


Must be a function of time and prox

if value hasnt changed by x amount in y time, then start decay
after every 250ms check if value has changed by enough

 */

pub async fn kays_decay(device_ip: &str, timeout: u64) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_millis(250)).await; // Make user defined
        let elapsed_time = Instant::now().duration_since(*DEVICE_LAST_SIGNAL_TIME.lock().unwrap().get(device_ip).unwrap_or(&Instant::now()));
        //println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);
        //println!("Kays Decay ");
        if elapsed_time >= Duration::from_secs(timeout-1) {
            //println!("Kays Decay ");
            giggletech_osc::send_data(device_ip, 0i32).await?;
            let mut device_last_signal_times = DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
            device_last_signal_times.insert(device_ip.to_string(), Instant::now());
        }
    }
}
