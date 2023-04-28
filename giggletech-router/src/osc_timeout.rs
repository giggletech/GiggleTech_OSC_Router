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

        if elapsed_time >= Duration::from_secs(5) {
            giggletech_osc::send_data(device_ip, 0i32).await?;

            let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
            *last_signal_time = Instant::now();
        }
    }
}
