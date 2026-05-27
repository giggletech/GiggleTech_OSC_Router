/*
    stop_pats.rs - Sending Stop Signal for GiggleTech Devices

    This module is responsible for sending a stop signal (`0i32`) to the device 
    five times in quick succession to ensure the motor stops.

    **Key Features:**

    1. **Sending Stop Signal**:
       - Sends the stop signal (`0i32`) to the device multiple times to ensure the motor stops.

    2. **Usage**:
       - Call `stop_pats` when you need to stop the device (e.g., proximity signal is `0.0`).
*/

use async_osc::Result;
use async_std::sync::Arc;
use std::sync::atomic::AtomicBool;

use crate::giggletech_osc;
use crate::config::DeviceConfig;
use crate::terminator;

/// Same stop sequence as proximity-off: halt terminator, five immediate stops, then periodic stop worker.
pub async fn stop_device_with_terminator(
    device_ip: &str,
    running: Arc<AtomicBool>,
) -> Result<()> {
    terminator::stop(running.clone()).await?;

    for _ in 0..5 {
        giggletech_osc::send_data(device_ip, 0i32).await?;
    }

    terminator::start(running.clone(), &Arc::new(device_ip.to_string())).await?;
    Ok(())
}

pub async fn stop_pats(device: DeviceConfig) -> Result<()> {
    let device_ip = device.device_uri.clone();
    let running = Arc::new(AtomicBool::new(false));

    crate::log_ui::log_line("Stopping pats...");

    stop_device_with_terminator(&device_ip, running).await
}
