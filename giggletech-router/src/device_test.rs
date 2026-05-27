//! Interactive device test slider: motor on while held, stop on release.

use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Mutex;

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::config;
use crate::giggletech_osc;
use crate::log_ui;

const MOTOR_SPEED_SCALE: f32 = 0.66;

static ACTIVE_SESSIONS: Lazy<Mutex<HashSet<String>>> =
  Lazy::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Deserialize)]
pub struct MotorPayload {
  pub ip: String,
  pub value: f32,
}

pub fn set_device_motor(ip: String, level: f32) {
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) = set_device_motor_async(&ip, level).await {
        log_ui::app_log(&format!("Device motor error ({}): {}", ip, e));
      }
    });
  });
}

pub fn stop_device(ip: String) {
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      stop_device_async(&ip).await;
    });
  });
}

async fn set_device_motor_async(ip: &str, level: f32) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Err("IP address is required.".to_string());
  }
  ip.parse::<IpAddr>()
    .map_err(|_| format!("Invalid IP address: {}", ip))?;

  let level = level.clamp(0.0, 1.0);
  if level <= 0.0 {
    return Ok(());
  }

  {
    let mut sessions = ACTIVE_SESSIONS.lock().unwrap();
    sessions.insert(ip.to_string());
  }

  let motor = motor_from_level(ip, level)?;
  giggletech_osc::send_data(ip, motor)
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

async fn stop_device_async(ip: &str) {
  let ip = ip.trim();
  if ip.is_empty() {
    return;
  }

  let was_active = {
    let mut sessions = ACTIVE_SESSIONS.lock().unwrap();
    sessions.remove(ip)
  };

  if !was_active {
    return;
  }

  for _ in 0..5 {
    let _ = giggletech_osc::send_data(ip, 0).await;
  }
}

fn motor_from_level(ip: &str, level: f32) -> Result<i32, String> {
  let (_global, devices) = config::load_config()?;
  if let Some(device) = devices.iter().find(|d| d.device_uri.as_str() == ip) {
    let mut headpat_tx = (((device.max_speed - device.min_speed) * level + device.min_speed)
      * MOTOR_SPEED_SCALE
      * device.speed_scale
      * 255.0)
      .round() as i32;
    if headpat_tx < device.start_tx {
      headpat_tx = device.start_tx;
    }
    return Ok(headpat_tx);
  }
  Ok((level * 255.0 * MOTOR_SPEED_SCALE).round().max(1.0) as i32)
}
