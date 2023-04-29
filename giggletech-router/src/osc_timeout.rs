// osc_timeout.rs

use std::sync::Mutex;
use std::time::{Duration, Instant};
use anyhow::Result;
use lazy_static::lazy_static;
use crate::giggletech_osc;


lazy_static! {
    pub static ref LAST_SIGNAL_TIME: Mutex<Instant> = Mutex::new(Instant::now());
}

pub async fn osc_timeout(device_ip: &str) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;
        let elapsed_time = Instant::now().duration_since(*LAST_SIGNAL_TIME.lock().unwrap());
        println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);
        if elapsed_time >= Duration::from_secs(5) {
            giggletech_osc::send_data(device_ip, 0i32).await?;

            let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
            *last_signal_time = Instant::now();
        }
    }
}

/* 
use std::collections::HashMap;
use std::time::{Duration, Instant};
use anyhow::Result;
use crate::giggletech_osc;

pub async fn osc_timeout(device_ips: Vec<String>) -> Result<()> {
    let mut last_signal_times: HashMap<String, Instant> = HashMap::new();

    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;

        for device_ip in &device_ips {
            let elapsed_time = Instant::now().duration_since(*last_signal_times.entry(device_ip.clone()).or_insert(Instant::now()));

            println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);

            if elapsed_time >= Duration::from_secs(5) {
                giggletech_osc::send_data(device_ip, 0i32).await?;
                last_signal_times.insert(device_ip.clone(), Instant::now());
            }
        }
    }
}
*/