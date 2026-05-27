//! Router lifecycle: run OSC loop and reload when config is saved.

use std::path::Path;
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
use crate::giggletech_osc;
use crate::device_test;
use crate::handle_proximity_parameter;
use crate::log_ui;
use crate::osc_timeout;
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

  task::spawn(async {
    loop {
      task::sleep(std::time::Duration::from_secs(300)).await;
      giggletech_osc::print_connection_stats().await;
    }
  });

  loop {
    match run_giggletech_session(&restart_rx).await {
      Ok(true) => {
        if !QUIET_RESTART.load(Ordering::SeqCst) {
          log_ui::app_log("Restarting router with updated configuration...");
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
    log_ui::app_log("Loading configuration...");
  }

  if !Path::new("config.yml").exists() {
    let error_msg = "Configuration file (config.yml) not found.";
    log_ui::app_log(error_msg);
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
      log_ui::app_log(&error_msg);
      return Err(async_osc::Error::Io(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        error_msg,
      )));
    }
  };

  let timeout = global_config.timeout;

  if !quiet_reload {
    log_ui::app_log("Configuration loaded successfully. Setting up sockets and timeouts.");
    crate::test_device_connectivity(&devices).await;
  }

  let mut rx_socket = giggletech_osc::setup_rx_socket(global_config.port_rx.to_string()).await?;

  for device in devices.iter() {
    let device_ip = device.device_uri.clone();
    let alive = session_alive.clone();
    task::spawn(async move {
      if let Err(e) = osc_timeout::osc_timeout(&device_ip, timeout, alive).await {
        log_ui::app_log(&format!("Timeout error for device {}: {}", device_ip, e));
      }
    });
  }

  if !quiet_reload {
    log_ui::app_log("Listening for OSC Packets...");
  }

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
          log_ui::app_log(&format!("Avatar Changed: {}", avatar_id));
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
