//! Send a short activation pulse to a device for UI testing.

use std::net::IpAddr;
use std::time::Duration;

use crate::config;
use crate::data_processing;
use crate::giggletech_osc;
use crate::log_ui;

/// Run a device test on a background thread (called from the UI / IPC handler).
pub fn spawn_device_test(ip: String) {
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) = test_device_async(&ip).await {
        log_ui::app_log(&format!("Device test failed ({}): {}", ip, e));
      }
    });
  });
}

async fn test_device_async(ip: &str) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Err("IP address is required.".to_string());
  }
  ip.parse::<IpAddr>()
    .map_err(|_| format!("Invalid IP address: {}", ip))?;

  let motor_on = motor_value_for_test(ip)?;

  log_ui::app_log(&format!("Testing device {} (motor={})...", ip, motor_on));

  giggletech_osc::send_data(ip, motor_on)
    .await
    .map_err(|e| format!("{}", e))?;

  async_std::task::sleep(Duration::from_millis(1500)).await;

  for _ in 0..5 {
    let _ = giggletech_osc::send_data(ip, 0).await;
  }

  log_ui::app_log(&format!("Device test complete for {}", ip));
  Ok(())
}

fn motor_value_for_test(ip: &str) -> Result<i32, String> {
  let (_global, devices) = config::load_config()?;
  if let Some(device) = devices.iter().find(|d| d.device_uri.as_str() == ip) {
    return Ok(data_processing::process_pat(1.0, device, 0.0));
  }
  Ok(20)
}
