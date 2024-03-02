use crate::giggletech_osc;
use anyhow::Result;
use lazy_static::lazy_static;
use std::collections::HashMap;
use async_std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

lazy_static! {
    pub static ref DEVICE_LAST_SIGNAL_TIME: Arc<Mutex<HashMap<String, Instant>>> =
        Arc::new(Mutex::new(HashMap::new()));
}

pub async fn osc_timeout(device_ip: &str, timeout: u64) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;
        
        let elapsed_time = {
            let lock = DEVICE_LAST_SIGNAL_TIME.lock().await;
            let device_last_signal_times = lock.get(device_ip);
            match device_last_signal_times {
                Some(last_signal) => Instant::now().duration_since(*last_signal),
                None => Duration::from_secs(0), // Assume no elapsed time if not found
            }
        };

        if elapsed_time >= Duration::from_secs(timeout) {
            giggletech_osc::send_data(device_ip, 0i32).await?;
            
            let mut lock = DEVICE_LAST_SIGNAL_TIME.lock().await;
            lock.insert(device_ip.to_string(), Instant::now());
        }
    }
}