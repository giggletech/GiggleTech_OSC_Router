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
use crate::giggletech_osc;
use crate::config::DeviceConfig;

pub async fn stop_pats(device: DeviceConfig) -> Result<()> {
    let device_ip = Arc::new(device.device_uri.clone());  // Use the device URI

    println!("Stopping pats...");

    // Send stop signal 5 times to ensure the motor stops
    for _ in 0..5 {
        giggletech_osc::send_data(&device_ip, 0i32).await?;  // Send stop signal
    }

    Ok(())
}
