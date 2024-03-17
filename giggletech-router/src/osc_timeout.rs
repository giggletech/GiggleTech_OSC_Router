// osc_timeout.rs

use std::collections::HashMap;
use async_std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use anyhow::Result;
use lazy_static::lazy_static;
use crate::giggletech_osc;

lazy_static! {
    pub static ref DEVICE_LAST_SIGNAL_TIME: Arc<Mutex<HashMap<String, Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub async fn osc_timeout(device_ip: &str, timeout: u64) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;
        let device_last_signal_times = DEVICE_LAST_SIGNAL_TIME.lock().await; 
        let elapsed_time = Instant::now().duration_since(*device_last_signal_times.get(device_ip).unwrap_or(&Instant::now()));
        //println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);
        if elapsed_time >= Duration::from_secs(timeout) {
            //println!("Timeout");
            giggletech_osc::send_data(device_ip, 0i32).await?;
            let mut device_last_signal_times = DEVICE_LAST_SIGNAL_TIME.lock().await;
            device_last_signal_times.insert(device_ip.to_string(), Instant::now());
        }
    }
}
