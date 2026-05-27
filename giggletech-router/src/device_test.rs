//! Interactive device test slider: motor on while held, same stop path as proximity-off.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use async_std::sync::{Arc, Mutex as AsyncMutex};
use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::config;
use crate::giggletech_osc;
use crate::log_ui;
use crate::stop_pats;
use crate::terminator;

const MOTOR_SPEED_SCALE: f32 = 0.66;

struct TestDeviceState {
  running: Arc<AtomicBool>,
  command_lock: Arc<AsyncMutex<()>>,
  /// Bumped on every stop request so stale motor IPC is ignored.
  epoch: Arc<AtomicU64>,
}

static TEST_DEVICES: Lazy<Mutex<HashMap<String, TestDeviceState>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
pub struct MotorPayload {
  pub ip: String,
  pub value: f32,
}

fn test_state(ip: &str) -> TestDeviceState {
  let mut devices = TEST_DEVICES.lock().unwrap();
  if let Some(state) = devices.get(ip) {
    return TestDeviceState {
      running: state.running.clone(),
      command_lock: state.command_lock.clone(),
      epoch: state.epoch.clone(),
    };
  }
  let state = TestDeviceState {
    running: Arc::new(AtomicBool::new(false)),
    command_lock: Arc::new(AsyncMutex::new(())),
    epoch: Arc::new(AtomicU64::new(0)),
  };
  let clone = TestDeviceState {
    running: state.running.clone(),
    command_lock: state.command_lock.clone(),
    epoch: state.epoch.clone(),
  };
  devices.insert(ip.to_string(), state);
  clone
}

pub fn set_device_motor(ip: String, level: f32) {
  let epoch_at_request = test_state(&ip).epoch.load(Ordering::SeqCst);
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) = set_device_motor_async(&ip, level, epoch_at_request).await {
        log_ui::app_log(&format!("Device motor error ({}): {}", ip, e));
      }
    });
  });
}

pub fn stop_device(ip: String) {
  let state = test_state(&ip);
  let epoch_at_request = state.epoch.fetch_add(1, Ordering::SeqCst) + 1;
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) = stop_device_async(&ip, epoch_at_request).await {
        log_ui::app_log(&format!("Device stop error ({}): {}", ip, e));
      }
    });
  });
}

async fn set_device_motor_async(
  ip: &str,
  level: f32,
  epoch_at_request: u64,
) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Err("IP address is required.".to_string());
  }
  ip.parse::<IpAddr>()
    .map_err(|_| format!("Invalid IP address: {}", ip))?;

  let state = test_state(ip);
  let _guard = state.command_lock.lock().await;

  if state.epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  let level = level.clamp(0.0, 1.0);
  if level <= 0.0 {
    let epoch_at_request = state.epoch.fetch_add(1, Ordering::SeqCst) + 1;
    return stop_device_async(ip, epoch_at_request).await;
  }

  // Stop the periodic stop worker before sending motor (same as handle_proximity entry).
  terminator::stop(state.running.clone())
    .await
    .map_err(|e| format!("{}", e))?;

  if state.epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  let motor = motor_from_level(ip, level)?;
  giggletech_osc::send_data(ip, motor)
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

async fn stop_device_async(ip: &str, epoch_at_request: u64) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Ok(());
  }

  let state = test_state(ip);
  let _guard = state.command_lock.lock().await;

  if state.epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  stop_pats::stop_device_with_terminator(ip, state.running.clone())
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
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
