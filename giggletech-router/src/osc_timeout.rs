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

const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor"; 
const TX_OSC_GIGGLESPARK: &str = "/motor"; 
const TX_OSC_COLLAR_1: &str = "/motor";
const TX_OSC_COLLAR_2: &str = "/motor_02";
const TX_OSC_COLLAR_3: &str = "/motor_03";
const TX_OSC_COLLAR_4: &str = "/motor_04";

pub async fn osc_timeout(device_ip: &str, timeout: u64) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;
        let elapsed_time = Instant::now().duration_since(*DEVICE_LAST_SIGNAL_TIME.lock().unwrap().get(device_ip).unwrap_or(&Instant::now()));
        //println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);
        if elapsed_time >= Duration::from_secs(timeout) {
            //println!("Timeout");
            giggletech_osc::send_data(device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;
            giggletech_osc::send_data(device_ip, TX_OSC_GIGGLESPARK, 0i32).await?;
            giggletech_osc::send_data(device_ip, TX_OSC_COLLAR_1, 0i32).await?;
            giggletech_osc::send_data(device_ip, TX_OSC_COLLAR_2, 0i32).await?;
            giggletech_osc::send_data(device_ip, TX_OSC_COLLAR_3, 0i32).await?;
            giggletech_osc::send_data(device_ip, TX_OSC_COLLAR_4, 0i32).await?;
            let mut device_last_signal_times = DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
            device_last_signal_times.insert(device_ip.to_string(), Instant::now());
        }
    }
}
