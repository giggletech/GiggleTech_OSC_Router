//! Interactive device test slider: motor on while held, same stop path as proximity-off.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Mutex;
use std::thread;

use async_std::sync::Arc;
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
  min_speed: f32,
  max_speed: f32,
  speed_scale: f32,
  start_tx: i32,
  proximity_parameter: String,
}

static MOTOR_CACHE: Lazy<Mutex<HashMap<String, MotorParams>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

enum TestCommand {
  Motor(f32),
  Stop,
}

struct TestDeviceState {
  running: Arc<AtomicBool>,
  epoch: Arc<AtomicU64>,
  tx: Sender<TestCommand>,
}

static TEST_DEVICES: Lazy<Mutex<HashMap<String, TestDeviceState>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Deserialize)]
pub struct MotorPayload {
  pub ip: String,
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
      epoch: state.epoch.clone(),
      tx: state.tx.clone(),
    };
  }

  let (tx, rx) = mpsc::channel();
  let running = Arc::new(AtomicBool::new(false));
  let epoch = Arc::new(AtomicU64::new(0));
  let worker_ip = ip.to_string();
  let worker_running = running.clone();
  let worker_epoch = epoch.clone();
  thread::spawn(move || {
    command_worker(&worker_ip, rx, worker_running, worker_epoch);
  });

  let state = TestDeviceState {
    running,
    epoch,
    tx,
  };
  devices.insert(ip.to_string(), TestDeviceState {
    running: state.running.clone(),
    epoch: state.epoch.clone(),
    tx: state.tx.clone(),
  });
  state
}

fn command_worker(
  ip: &str,
  rx: Receiver<TestCommand>,
  running: Arc<AtomicBool>,
  epoch: Arc<AtomicU64>,
) {
  while let Ok(first) = rx.recv() {
    let mut cmd = first;
    'drive: loop {
      while let Ok(next) = rx.try_recv() {
        if matches!(next, TestCommand::Stop) {
          cmd = TestCommand::Stop;
          break;
        }
        cmd = next;
      }

      let result = async_std::task::block_on(async {
        match cmd {
          TestCommand::Motor(level) => set_device_motor_async(ip, level, &running, &epoch).await,
          TestCommand::Stop => {
            let epoch_at_request = epoch.load(Ordering::SeqCst);
            stop_device_async(ip, epoch_at_request, &running, &epoch).await
          }
        }
      });

      if let Err(e) = result {
        log_ui::status(&format!("Device test error ({}): {}", ip, e));
      }

      if matches!(cmd, TestCommand::Stop) {
        break;
      }

      cmd = TestCommand::Stop;
      while let Ok(next) = rx.try_recv() {
        match next {
          TestCommand::Stop => break,
          TestCommand::Motor(level) => {
            cmd = TestCommand::Motor(level);
          }
        }
      }
      if matches!(cmd, TestCommand::Motor(_)) {
        continue 'drive;
      }
      break;
    }
  }
}

/// Stops any test-slider periodic stop workers (e.g. after a prior session bug or reload).
pub fn stop_all_test_terminators() {
  let devices = TEST_DEVICES.lock().unwrap();
  for state in devices.values() {
    state.running.store(false, Ordering::SeqCst);
  }
}

pub fn set_device_motor(ip: String, level: f32) {
  let state = test_state(ip.trim());
  let _ = state.tx.send(TestCommand::Motor(level));
}

pub fn stop_device(ip: String) {
  let ip = ip.trim().to_string();
  let state = test_state(&ip);
  epoch_fetch_stop(&state.epoch);
  let _ = state.tx.send(TestCommand::Stop);
}

fn epoch_fetch_stop(epoch: &AtomicU64) {
  epoch.fetch_add(1, Ordering::SeqCst);
}

async fn set_device_motor_async(
  ip: &str,
  level: f32,
  running: &Arc<AtomicBool>,
  epoch: &Arc<AtomicU64>,
) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Err("IP address is required.".to_string());
  }
  ip.parse::<IpAddr>()
    .map_err(|_| format!("Invalid IP address: {}", ip))?;

  let epoch_at_request = epoch.load(Ordering::SeqCst);

  let level = level.clamp(0.0, 1.0);
  if level <= 0.0 {
    return stop_device_async(ip, epoch.load(Ordering::SeqCst), running, epoch).await;
  }

  if running.load(Ordering::SeqCst) {
    terminator::stop(running.clone())
      .await
      .map_err(|e| format!("{}", e))?;
  }

  if epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  let (motor, _params) = motor_from_level(ip, level)?;
  giggletech_osc::send_data(ip, motor)
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

async fn stop_device_async(
  ip: &str,
  epoch_at_request: u64,
  running: &Arc<AtomicBool>,
  epoch: &Arc<AtomicU64>,
) -> Result<(), String> {
  let ip = ip.trim();
  if ip.is_empty() {
    return Ok(());
  }

  if epoch.load(Ordering::SeqCst) != epoch_at_request {
    return Ok(());
  }

  stop_pats::stop_device_immediate(ip, running.clone())
    .await
    .map_err(|e| format!("{}", e))?;
  Ok(())
}

fn motor_from_level(ip: &str, level: f32) -> Result<(i32, MotorParams), String> {
  let params = motor_params(ip)?;
  let mut headpat_tx = (((params.max_speed - params.min_speed) * level + params.min_speed)
    * MOTOR_SPEED_SCALE
    * params.speed_scale
    * 255.0)
    .round() as i32;
  if headpat_tx < params.start_tx {
    headpat_tx = params.start_tx;
  }
  Ok((headpat_tx, params))
}

fn motor_params(ip: &str) -> Result<MotorParams, String> {
  if let Some(cached) = MOTOR_CACHE.lock().unwrap().get(ip).cloned() {
    return Ok(cached);
  }

  let (_global, devices) = config::load_config_quiet()?;
  let params = if let Some(device) = devices.iter().find(|d| d.device_uri.as_str() == ip) {
    MotorParams {
      min_speed: device.min_speed,
      max_speed: device.max_speed,
      speed_scale: device.speed_scale,
      start_tx: device.start_tx,
      proximity_parameter: device.proximity_parameter.to_string(),
    }
  } else {
    MotorParams {
      min_speed: 0.0,
      max_speed: 1.0,
      speed_scale: 1.0,
      start_tx: 1,
      proximity_parameter: "test".to_string(),
    }
  };

  MOTOR_CACHE
    .lock()
    .unwrap()
    .insert(ip.to_string(), params.clone());
  Ok(params)
}
