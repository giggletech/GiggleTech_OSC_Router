//! Interactive device test slider: motor on while held, same stop path as proximity-off.
//!
//! Slider sends `value` 0.0–1.0 from the UI; that maps to motor output 0–100% of the
//! device's configured max speed (0% = off, 100% = full max).

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

#[derive(Clone)]
struct MotorParams {
  max_speed: f32,
  speed_scale: f32,
}

static MOTOR_CACHE: Lazy<Mutex<HashMap<String, MotorParams>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

struct TestDeviceState {
  running: Arc<AtomicBool>,
  command_lock: Arc<AsyncMutex<()>>,
  epoch: Arc<AtomicU64>,
}

static TEST_DEVICES: Lazy<Mutex<HashMap<String, TestDeviceState>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
pub struct MotorPayload {
  pub ip: String,
  /// UI slider position 0.0–1.0 (= output 0–100% of configured max speed).
  pub value: f32,
}

pub fn invalidate_motor_cache() {
  MOTOR_CACHE.lock().unwrap().clear();
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

pub fn stop_all_test_terminators() {
  let ips: Vec<String> = TEST_DEVICES
    .lock()
    .unwrap()
    .keys()
    .cloned()
    .collect();
  for ip in ips {
    stop_device(ip);
  }
}

/// `level` is the UI slider value 0.0–1.0 (output percent ÷ 100).
pub fn set_device_motor(ip: String, level: f32) {
  let output_percent = level.clamp(0.0, 1.0) * 100.0;
  set_device_motor_output(ip, output_percent);
}

fn set_device_motor_output(ip: String, output_percent: f32) {
  let ip = ip.trim().to_string();
  if ip.is_empty() {
    return;
  }
  let state = test_state(&ip);
  let epoch_at_request = state.epoch.load(Ordering::SeqCst);
  let command_lock = state.command_lock.clone();
  let running = state.running.clone();
  let epoch = state.epoch.clone();
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) = set_device_motor_async(
        &ip,
        output_percent,
        epoch_at_request,
        &command_lock,
        &running,
        &epoch,
      )
      .await
      {
        log_ui::status(&format!("Device test error ({}): {}", ip, e));
      }
    });
  });
}

pub fn stop_device(ip: String) {
  let ip = ip.trim().to_string();
  if ip.is_empty() {
    return;
  }
  let state = test_state(&ip);
  let epoch_at_request = state.epoch.fetch_add(1, Ordering::SeqCst) + 1;
  let command_lock = state.command_lock.clone();
  let running = state.running.clone();
  let epoch = state.epoch.clone();
  std::thread::spawn(move || {
    async_std::task::block_on(async move {
      if let Err(e) =
        stop_device_async(&ip, epoch_at_request, &command_lock, &running, &epoch).await
      {
        log_ui::status(&format!("Device test error ({}): {}", ip, e));
      }
    });
  });
}

async fn set_device_motor_async(
  ip: &str,
  output_percent: f32,
  epoch_at_request: u64,
  command_lock: &Arc<AsyncMutex<()>>,
  running: &Arc<AtomicBool>,
  epoch: &Arc<AtomicU64>,
) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Err("IP address is required.".to_string());
  }
  ip.parse::<IpAddr>()
    .map_err(|_| format!("Invalid IP address: {}", ip))?;

  let _guard = command_lock.lock().await;

  if epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  let output_percent = output_percent.clamp(0.0, 100.0);
  if output_percent <= 0.0 {
    let stop_epoch = epoch.fetch_add(1, Ordering::SeqCst) + 1;
    drop(_guard);
    return stop_device_async(ip, stop_epoch, command_lock, running, epoch).await;
  }

  if running.load(Ordering::SeqCst) {
    terminator::stop(running.clone())
      .await
      .map_err(|e| format!("{}", e))?;
  }

  if epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  let motor = motor_tx_from_output_percent(ip, output_percent)?;
  giggletech_osc::send_data(ip, motor)
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

async fn stop_device_async(
  ip: &str,
  epoch_at_request: u64,
  command_lock: &Arc<AsyncMutex<()>>,
  running: &Arc<AtomicBool>,
  epoch: &Arc<AtomicU64>,
) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Ok(());
  }

  let _guard = command_lock.lock().await;

  if epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  stop_pats::stop_device_immediate(ip, running.clone())
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

/// 0% = off; 100% = full output at the device's configured `max_speed`.
fn motor_tx_from_output_percent(ip: &str, output_percent: f32) -> Result<i32, String> {
  let params = motor_params(ip)?;
  let level = (output_percent / 100.0).clamp(0.0, 1.0);
  Ok((params.max_speed * level * MOTOR_SPEED_SCALE * params.speed_scale * 255.0).round() as i32)
}

fn motor_params(ip: &str) -> Result<MotorParams, String> {
  if let Some(cached) = MOTOR_CACHE.lock().unwrap().get(ip).cloned() {
    return Ok(cached);
  }

  let (_global, devices) = config::load_config_quiet()?;
  let params = if let Some(device) = devices.iter().find(|d| d.device_uri.as_str() == ip) {
    MotorParams {
      max_speed: device.max_speed,
      speed_scale: device.speed_scale,
    }
  } else {
    MotorParams {
      max_speed: 1.0,
      speed_scale: 1.0,
    }
  };

  MOTOR_CACHE
    .lock()
    .unwrap()
    .insert(ip.to_string(), params.clone());
  Ok(params)
}
