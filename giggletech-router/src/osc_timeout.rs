/*
    osc_timeout.rs - Timeout Management for OSC Devices

    This module is responsible for managing timeouts for devices communicating over OSC (Open Sound Control).
    It monitors how long it's been since each device has sent a signal and, if the device exceeds the specified timeout,
    it sends a stop signal to the device.

    **Key Features:**

    1. **Device Signal Tracking**:
       - Uses a global `DEVICE_LAST_SIGNAL_TIME` hash map (wrapped in `Arc<Mutex>`) to store the last time each device sent a signal.
       - This ensures each device's signal time is updated and shared across the system safely.

    2. **Timeout Loop (`osc_timeout`)**:
       - Runs an asynchronous loop that periodically checks how long it's been since a device last sent a signal.
       - If the time elapsed exceeds the specified timeout duration, the module sends a stop signal (`0`) to the device via OSC.
       - Resets the last signal time to prevent repeated stops during the timeout period.

    **Usage**:
    - The function `osc_timeout` is typically called for each device in the system, running concurrently to monitor signal activity.
    - It ensures that devices stop operating (such as motors) if no signal is received within the timeout window, preventing runaway behavior.

*/


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

pub async fn osc_timeout(device_ip: &str, timeout: u64) -> Result<()> {
    loop {
        async_std::task::sleep(Duration::from_secs(1)).await;
        let elapsed_time = Instant::now().duration_since(*DEVICE_LAST_SIGNAL_TIME.lock().unwrap().get(device_ip).unwrap_or(&Instant::now()));
        //println!("Device Ip {} Elapsed Time {:?}", device_ip, elapsed_time);
        if elapsed_time >= Duration::from_secs(timeout) {
            //println!("Timeout");
            giggletech_osc::send_data(device_ip, 0i32).await?;
            let mut device_last_signal_times = DEVICE_LAST_SIGNAL_TIME.lock().unwrap();
            device_last_signal_times.insert(device_ip.to_string(), Instant::now());
        }
    }
}
