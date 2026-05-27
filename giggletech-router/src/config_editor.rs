//! Load/save device entries in config.yml for the settings UI.

use std::fs;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::config::config_validator::{load_config, Device};
use crate::log_ui;

pub const CONFIG_PATH: &str = "config.yml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorDevice {
  pub ip: String,
  pub proximity_parameter: String,
  /// Motor max speed limit as a percentage (e.g. 5–100).
  pub max_speed: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
  pub devices: Vec<EditorDevice>,
  pub min_speed: u32,
  pub max_speed_cap: u32,
}

pub fn load_editor_json() -> Result<String, String> {
  let state = load_editor_state()?;
  serde_json::to_string(&state).map_err(|e| e.to_string())
}

pub fn load_editor_state() -> Result<EditorState, String> {
  let cfg = load_config(CONFIG_PATH)?;
  let default_max = cfg.setup.default_max_speed;
  let min_speed = cfg.setup.default_min_speed;

  Ok(EditorState {
    devices: cfg
      .devices
      .into_iter()
      .map(|d| EditorDevice {
        ip: d.ip,
        proximity_parameter: strip_proximity_prefix(d.proximity_parameter),
        max_speed: d.max_speed.unwrap_or(default_max).max(min_speed),
      })
      .collect(),
    min_speed,
    max_speed_cap: default_max.max(min_speed),
  })
}

#[derive(Debug, Deserialize)]
struct SaveRequest {
  #[serde(flatten)]
  state: EditorState,
  #[serde(default)]
  quiet: bool,
}

pub fn save_editor_json(json: &str) -> Result<bool, String> {
  let req: SaveRequest =
    serde_json::from_str(json).map_err(|e| format!("Invalid config data: {}", e))?;
  save_editor_state(&req.state, req.quiet)?;
  Ok(req.quiet)
}

pub fn save_editor_state(state: &EditorState, quiet: bool) -> Result<(), String> {
  if state.devices.is_empty() {
    return Err("At least one device is required.".to_string());
  }

  for (i, device) in state.devices.iter().enumerate() {
    if device.ip.trim().is_empty() {
      return Err(format!("Device {}: IP is required.", i + 1));
    }
    device
      .ip
      .trim()
      .parse::<IpAddr>()
      .map_err(|_| format!("Device {}: invalid IP address.", i + 1))?;
    if device.proximity_parameter.trim().is_empty() {
      return Err(format!("Device {}: proximity parameter is required.", i + 1));
    }
  }

  let mut cfg = load_config(CONFIG_PATH)?;
  let min_speed = cfg.setup.default_min_speed;
  let default_max = cfg.setup.default_max_speed;

  for (i, device) in state.devices.iter().enumerate() {
    if device.max_speed < min_speed {
      return Err(format!(
        "Device {}: max speed must be at least {}%.",
        i + 1,
        min_speed
      ));
    }
    if device.max_speed > 100 {
      return Err(format!("Device {}: max speed cannot exceed 100%.", i + 1));
    }
  }
  let existing = cfg.devices.clone();

  cfg.devices = state
    .devices
    .iter()
    .enumerate()
    .map(|(i, ed)| {
      if i < existing.len() {
        let mut device = existing[i].clone();
        device.ip = ed.ip.trim().to_string();
        device.proximity_parameter = normalize_proximity_parameter(&ed.proximity_parameter);
        device.max_speed = if ed.max_speed == default_max {
          None
        } else {
          Some(ed.max_speed)
        };
        device
      } else {
        Device {
          ip: ed.ip.trim().to_string(),
          proximity_parameter: normalize_proximity_parameter(&ed.proximity_parameter),
          max_speed: if ed.max_speed == default_max {
            None
          } else {
            Some(ed.max_speed)
          },
          speed_scale: None,
          max_speed_parameter: None,
          use_velocity_control: None,
          outer_proximity: None,
          inner_proximity: None,
          velocity_scalar: None,
        }
      }
    })
    .collect();

  let yaml =
    serde_yaml::to_string(&cfg).map_err(|e| format!("Failed to serialize config: {}", e))?;
  fs::write(CONFIG_PATH, yaml).map_err(|e| format!("Failed to write config.yml: {}", e))?;
  crate::device_test::invalidate_motor_cache();
  if quiet {
    crate::router::request_restart_quiet();
  } else {
    log_ui::app_log("Configuration saved. Reloading router...");
    crate::router::request_restart();
  }
  Ok(())
}

fn strip_proximity_prefix(s: String) -> String {
  const PREFIX: &str = "/avatar/parameters/";
  if s.starts_with(PREFIX) {
    s[PREFIX.len()..].to_string()
  } else {
    s.trim_start_matches('/').to_string()
  }
}

fn normalize_proximity_parameter(s: &str) -> String {
  let s = s.trim();
  if s.starts_with("/avatar/parameters/") {
    return s["/avatar/parameters/".len()..].to_string();
  }
  s.trim_start_matches('/').to_string()
}
