//! Router lifecycle: run OSC loop and reload when config is saved.

use std::sync::atomic::{AtomicBool, Ordering};

static QUIET_RESTART: AtomicBool = AtomicBool::new(false);
use std::sync::Mutex;

use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{
  channel::{self, Receiver, Sender},
  stream::StreamExt,
  sync::Arc,
  task,
};
use futures::{FutureExt, select};
use once_cell::sync::OnceCell;

use crate::config;
use crate::device_ping;
use crate::giggletech_osc;
use crate::device_test;
use crate::handle_proximity_parameter;
use crate::log_ui;
use crate::osc_timeout;
use crate::vrc_osc;
static RESTART_TX: OnceCell<Mutex<Option<Sender<()>>>> = OnceCell::new();

/// Register the channel used to request a config reload. Call once before `run_giggletech_loop`.
pub fn init_restart_channel() -> Receiver<()> {
  let (tx, rx) = channel::bounded(1);
  let _ = RESTART_TX.set(Mutex::new(Some(tx)));
  rx
}

/// Ask the router to stop the current session and reload `config.yml`.
pub fn request_restart() {
  QUIET_RESTART.store(false, Ordering::SeqCst);
  send_restart_signal();
}

/// Reload `config.yml` without printing the full startup banner (e.g. max-speed slider save).
pub fn request_restart_quiet() {
  QUIET_RESTART.store(true, Ordering::SeqCst);
  send_restart_signal();
}

fn send_restart_signal() {
  if let Some(mutex) = RESTART_TX.get() {
    if let Ok(guard) = mutex.lock() {
      if let Some(tx) = guard.as_ref() {
        let _ = tx.try_send(());
      }
    }
  }
}

/// Runs the router until the process exits. Restarts automatically when `request_restart` is called.
pub async fn run_giggletech_loop(restart_rx: Receiver<()>) -> Result<()> {
  giggletech_osc::start_connection_manager().await;

  loop {
    match run_giggletech_session(&restart_rx).await {
      Ok(true) => {
        if !QUIET_RESTART.load(Ordering::SeqCst) {
          log_ui::status("Restarting with updated configuration...");
        }
        while restart_rx.try_recv().is_ok() {}
      }
      Ok(false) => break,
      Err(e) => return Err(e),
    }
  }

  Ok(())
}

/// One router session. Returns `Ok(true)` when reload was requested.
async fn run_giggletech_session(restart_rx: &Receiver<()>) -> Result<bool> {
  let session_alive = Arc::new(AtomicBool::new(true));
  let running = Arc::new(AtomicBool::new(false));

  device_test::stop_all_test_terminators();

  let quiet_reload = QUIET_RESTART.swap(false, Ordering::SeqCst);

  if !quiet_reload {
    log_ui::status("Loading configuration...");
  }

  let config_path = config::config_file_path();
  if !config_path.exists() {
    let error_msg = format!("Configuration file not found: {}", config_path.display());
    log_ui::status(&error_msg);
    return Err(async_osc::Error::Io(std::io::Error::new(
      std::io::ErrorKind::NotFound,
      error_msg,
    )));
  }

  let (global_config, mut devices) = match if quiet_reload {
    config::load_config_quiet()
  } else {
    config::load_config()
  } {
    Ok(config) => config,
    Err(e) => {
      let error_msg = format!("Config file error: {}", e);
      log_ui::status(&error_msg);
      return Err(async_osc::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        error_msg,
      )));
    }
  };

  let timeout = global_config.timeout;

  // Let the output window motor bars reflect the actual motor TX we send to each device.
  // This covers stop/timeout/test paths (anything that uses `giggletech_osc::send_data`).
  log_ui::set_motor_ui_devices(
    devices
      .iter()
      .map(|d| {
        // Match the actual integer TX rounding used by motor output.
        let max_tx = (d.max_speed as f32 * 0.66 * d.speed_scale * 255.0).round();
        (d.device_uri.to_string(), d.proximity_parameter.as_ref().clone(), max_tx)
      })
      .collect(),
  );

  let ping_interval_ms = online_monitor_interval_ms(&global_config);
  device_ping::monitor().sync_ips(devices.iter().map(|d| d.device_uri.as_ref().clone()));
  device_ping::monitor().set_interval_ms(ping_interval_ms);
  spawn_online_monitor(session_alive.clone(), &global_config, &devices, ping_interval_ms);

  let mut rx_socket = giggletech_osc::setup_rx_socket(global_config.port_rx.to_string()).await?;

  for device in devices.iter() {
    let device_ip = device.device_uri.clone();
    let alive = session_alive.clone();
    task::spawn(async move {
      if let Err(e) = osc_timeout::osc_timeout(&device_ip, timeout, alive).await {
        log_ui::status(&format!("Timeout error for device {}: {}", device_ip, e));
      }
    });
  }

  log_ui::status(&format!(
    "Listening for OSC on port {} (timeout {}s)",
    global_config.port_rx, timeout
  ));

  let mut should_restart = false;

  loop {
    select! {
      _ = restart_rx.recv().fuse() => {
        should_restart = true;
        break;
      }
      packet = rx_socket.next().fuse() => {
        match packet {
          None => break,
          Some(Ok((packet, _peer_addr))) => {
            if !process_osc_packet(
              packet,
              &global_config,
              &mut devices,
              running.clone(),
            )
            .await?
            {
              continue;
            }
          }
          Some(Err(e)) => return Err(e),
        }
      }
    }
  }

  session_alive.store(false, Ordering::SeqCst);
  running.store(false, Ordering::SeqCst);

  Ok(should_restart)
}

fn online_monitor_interval_ms(global_config: &config::GlobalConfig) -> u64 {
  let broadcast_secs = global_config.online_status_broadcast_seconds;
  if broadcast_secs > 0 {
    broadcast_secs * 1000
  } else {
    5000
  }
}

fn spawn_online_monitor(
  alive: Arc<AtomicBool>,
  global_config: &config::GlobalConfig,
  devices: &[config::DeviceConfig],
  poll_interval_ms: u64,
) {
  let broadcast_secs = global_config.online_status_broadcast_seconds;
  let vrc_targets: Vec<(String, String)> = devices
    .iter()
    .filter_map(|d| {
      d.online_parameter.as_ref().map(|p| {
        (d.device_uri.as_ref().clone(), p.as_ref().clone())
      })
    })
    .collect();

  if vrc_targets.is_empty() {
    return;
  }

  let monitor = device_ping::monitor();

  task::spawn(async move {
    use std::collections::HashMap;
    let mut last: HashMap<String, bool> = HashMap::new();

    while alive.load(Ordering::SeqCst) {
      for (ip, param) in vrc_targets.iter() {
        let Some(current) = monitor.get(ip) else {
          continue;
        };
        let prev = last.insert(ip.clone(), current);
        let changed = prev != Some(current);

        if !changed && prev.is_some() && broadcast_secs == 0 {
          continue;
        }

        let pulse = changed && current;
        if vrc_osc::send_avatar_parameter(param, current, pulse)
          .await
          .is_ok()
          && changed
        {
          log_ui::status(&format!("{} {}", param, current));
        }
      }

      task::sleep(std::time::Duration::from_millis(poll_interval_ms)).await;
    }
  });
}

async fn process_osc_packet(
  packet: OscPacket,
  global_config: &config::GlobalConfig,
  devices: &mut [config::DeviceConfig],
  running: Arc<AtomicBool>,
) -> Result<bool> {
  match packet {
    OscPacket::Bundle(_) => {}
    OscPacket::Message(message) => {
      let (address, osc_value) = message.as_tuple();

      if address == "/avatar/change" {
        if let Some(OscType::String(avatar_id)) = osc_value.first() {
          log_ui::status(&format!("Avatar changed: {}", avatar_id));
        }
        return Ok(true);
      }

      let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float() {
        Some(v) => v,
        None => return Ok(true),
      };

      for device in devices.iter_mut() {
        if address == *device.max_speed_parameter {
          crate::data_processing::print_speed_limit(value);
          device.max_speed = value.max(global_config.minimum_max_speed);
        } else if address == *device.proximity_parameter {
          handle_proximity_parameter::handle_proximity_parameter(
            running.clone(),
            value,
            device.clone(),
          )
          .await?;
        }
      }
    }
  }
  Ok(true)
}
