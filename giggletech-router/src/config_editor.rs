//! Load/save device entries in config.yml for the settings UI.

use std::fs;
use std::net::IpAddr;

use serde::{Deserialize, Serialize};

use crate::config::config_file_path;
use crate::config::config_validator::{load_config, Device};
use crate::config::{default_online_parameter_short, effective_device_name};
use crate::log_ui;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorDevice {
  /// Display name in the config UI (optional).
  #[serde(default)]
  pub name: String,
  pub ip: String,
  pub proximity_parameter: String,
  /// VRChat parameter name for live max speed (without `/avatar/parameters/` prefix).
  #[serde(default)]
  pub max_speed_parameter: String,
  /// Motor max speed limit as a percentage (e.g. 5–100).
  pub max_speed: u32,
  /// When true, motor follows approach velocity; when false, proximity level.
  #[serde(default)]
  pub use_velocity_control: bool,
  /// When true (and velocity control on), motor also fires when proximity decreases.
  #[serde(default)]
  pub velocity_on_prox_drop: bool,
  /// Velocity band far edge — proximity must be above this (`outer_proximity` in config.yml).
  #[serde(default)]
  pub outer_proximity: f32,
  /// Velocity band close edge — proximity must be below this (`inner_proximity` in config.yml).
  #[serde(default)]
  pub inner_proximity: f32,
  #[serde(default)]
  pub velocity_scalar: u32,
  /// Soft cap for velocity after scaling (larger = less damping).
  #[serde(default)]
  pub velocity_softcap: u32,
  /// EMA smoothing time constant for velocity control (milliseconds).
  #[serde(default)]
  pub velocity_smoothing_ms: u32,
}

/// `port_rx` in config.yml: `"OSCQuery"` or a UDP port number (e.g. `"9001"`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditorState {
  pub devices: Vec<EditorDevice>,
  pub min_speed: u32,
  pub max_speed_cap: u32,
  #[serde(default = "default_port_rx")]
  pub port_rx: String,
  /// Default for new devices (from `setup.default_use_velocity_control`).
  #[serde(default)]
  pub default_use_velocity_control: bool,
  #[serde(default)]
  pub default_velocity_on_prox_drop: bool,
  #[serde(default)]
  pub default_outer_proximity: f32,
  #[serde(default)]
  pub default_inner_proximity: f32,
  #[serde(default)]
  pub default_velocity_scalar: u32,
  #[serde(default)]
  pub default_velocity_softcap: u32,
  #[serde(default)]
  pub default_velocity_smoothing_ms: u32,
  /// Global default from `setup.default_max_speed_parameter`.
  #[serde(default = "default_max_speed_parameter")]
  pub default_max_speed_parameter: String,
}

fn default_max_speed_parameter() -> String {
  "max_speed".to_string()
}

fn default_port_rx() -> String {
  "OSCQuery".to_string()
}

fn normalize_port_rx_for_editor(port_rx: &str) -> String {
  let s = port_rx.trim().trim_matches('\'').trim_matches('"');
  if s.eq_ignore_ascii_case("OSCQuery") {
    "OSCQuery".to_string()
  } else {
    s.to_string()
  }
}

pub fn load_editor_json() -> Result<String, String> {
  let state = load_editor_state()?;
  serde_json::to_string(&state).map_err(|e| e.to_string())
}

pub fn load_editor_state() -> Result<EditorState, String> {
  let cfg = load_config(config_file_path())?;
  let default_max = cfg.setup.default_max_speed;
  let min_speed = cfg.setup.default_min_speed;
  let default_use_velocity_control = cfg.setup.default_use_velocity_control;
  let default_velocity_on_prox_drop = cfg.setup.default_velocity_on_prox_drop;
  let default_outer_proximity = cfg.setup.default_outer_proximity as f32;
  let default_inner_proximity = cfg.setup.default_inner_proximity as f32;
  let default_velocity_scalar = cfg.setup.default_velocity_scalar;
  let default_velocity_softcap = cfg.setup.default_velocity_softcap;
  let default_velocity_smoothing_ms = cfg.setup.default_velocity_smoothing_ms;
  let default_max_speed_parameter = cfg.setup.default_max_speed_parameter.clone();

  Ok(EditorState {
    devices: cfg
      .devices
      .into_iter()
      .enumerate()
      .map(|(i, d)| EditorDevice {
        name: effective_device_name(i, &d.name.unwrap_or_default()),
        ip: d.ip,
        proximity_parameter: strip_avatar_parameter_short(&d.proximity_parameter),
        max_speed_parameter: editor_max_speed_parameter_display(d.max_speed_parameter.as_deref()),
        max_speed: d.max_speed.unwrap_or(default_max).max(min_speed),
        use_velocity_control: d
          .use_velocity_control
          .unwrap_or(default_use_velocity_control),
        velocity_on_prox_drop: d
          .velocity_on_prox_drop
          .unwrap_or(default_velocity_on_prox_drop),
        outer_proximity: d
          .outer_proximity
          .map(|x| x as f32)
          .unwrap_or(default_outer_proximity),
        inner_proximity: d
          .inner_proximity
          .map(|x| x as f32)
          .unwrap_or(default_inner_proximity),
        velocity_scalar: d.velocity_scalar.unwrap_or(default_velocity_scalar),
        velocity_softcap: d.velocity_softcap.unwrap_or(default_velocity_softcap),
        velocity_smoothing_ms: d
          .velocity_smoothing_ms
          .unwrap_or(default_velocity_smoothing_ms),
      })
      .collect(),
    min_speed,
    max_speed_cap: default_max.max(min_speed),
    port_rx: normalize_port_rx_for_editor(&cfg.setup.port_rx),
    default_use_velocity_control,
    default_velocity_on_prox_drop,
    default_outer_proximity,
    default_inner_proximity,
    default_velocity_scalar,
    default_velocity_softcap,
    default_velocity_smoothing_ms,
    default_max_speed_parameter,
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

  let mut cfg = load_config(config_file_path())?;
  let min_speed = cfg.setup.default_min_speed;
  let default_max = cfg.setup.default_max_speed;

  for (i, device) in state.devices.iter().enumerate() {
    if device.max_speed < min_speed {
      return Err(format!(
        "Device {}: power must be at least {}%.",
        i + 1,
        min_speed
      ));
    }
    if device.max_speed > 100 {
      return Err(format!("Device {}: power cannot exceed 100%.", i + 1));
    }
    if device.inner_proximity <= device.outer_proximity {
      return Err(format!(
        "Device {}: inner proximity must be greater than outer.",
        i + 1
      ));
    }
    if device.velocity_scalar < 1 || device.velocity_scalar > 100 {
      return Err(format!(
        "Device {}: velocity sensitivity must be between 1 and 100.",
        i + 1
      ));
    }
    if device.velocity_softcap < 1 || device.velocity_softcap > 100 {
      return Err(format!(
        "Device {}: velocity damping must be between 1 and 100.",
        i + 1
      ));
    }
  }
  let existing = cfg.devices.clone();
  let default_use_velocity_control = cfg.setup.default_use_velocity_control;
  let default_velocity_on_prox_drop = cfg.setup.default_velocity_on_prox_drop;
  let default_outer_proximity = cfg.setup.default_outer_proximity;
  let default_inner_proximity = cfg.setup.default_inner_proximity;
  let default_velocity_scalar = cfg.setup.default_velocity_scalar;
  let default_velocity_softcap = cfg.setup.default_velocity_softcap;
  let default_velocity_smoothing_ms = cfg.setup.default_velocity_smoothing_ms;

  cfg.devices = state
    .devices
    .iter()
    .enumerate()
    .map(|(i, ed)| {
      if i < existing.len() {
        let mut device = existing[i].clone();
        device.name = name_for_yaml(&effective_device_name(i, &ed.name));
        device.ip = ed.ip.trim().to_string();
        device.proximity_parameter = normalize_avatar_parameter_short(&ed.proximity_parameter);
        device.max_speed_parameter = optional_max_speed_parameter(&ed.max_speed_parameter);
        device.max_speed = if ed.max_speed == default_max {
          None
        } else {
          Some(ed.max_speed)
        };
        device.use_velocity_control = if ed.use_velocity_control == default_use_velocity_control {
          None
        } else {
          Some(ed.use_velocity_control)
        };
        device.velocity_on_prox_drop =
          if ed.velocity_on_prox_drop == default_velocity_on_prox_drop {
            None
          } else {
            Some(ed.velocity_on_prox_drop)
          };
        device.outer_proximity =
          optional_f64(ed.outer_proximity, default_outer_proximity);
        device.inner_proximity =
          optional_f64(ed.inner_proximity, default_inner_proximity);
        device.velocity_scalar = optional_u32(ed.velocity_scalar, default_velocity_scalar);
        device.velocity_softcap = optional_u32(ed.velocity_softcap, default_velocity_softcap);
        device.velocity_smoothing_ms =
          optional_u32(ed.velocity_smoothing_ms, default_velocity_smoothing_ms);
        let previous_name = existing[i].name.as_deref().unwrap_or("");
        device.online_parameter = Some(resolve_online_parameter_for_save(
          i,
          &ed.name,
          previous_name,
          device.online_parameter.as_deref(),
        ));
        device
      } else {
        Device {
          name: name_for_yaml(&effective_device_name(i, &ed.name)),
          ip: ed.ip.trim().to_string(),
          proximity_parameter: normalize_avatar_parameter_short(&ed.proximity_parameter),
          online_parameter: Some(resolve_online_parameter_for_save(
            i,
            &ed.name,
            "",
            None,
          )),
          max_speed: if ed.max_speed == default_max {
            None
          } else {
            Some(ed.max_speed)
          },
          speed_scale: None,
          max_speed_parameter: optional_max_speed_parameter(&ed.max_speed_parameter),
          use_velocity_control: if ed.use_velocity_control == default_use_velocity_control {
            None
          } else {
            Some(ed.use_velocity_control)
          },
          velocity_on_prox_drop: if ed.velocity_on_prox_drop == default_velocity_on_prox_drop {
            None
          } else {
            Some(ed.velocity_on_prox_drop)
          },
          outer_proximity: optional_f64(ed.outer_proximity, default_outer_proximity),
          inner_proximity: optional_f64(ed.inner_proximity, default_inner_proximity),
          velocity_scalar: optional_u32(ed.velocity_scalar, default_velocity_scalar),
          velocity_softcap: optional_u32(ed.velocity_softcap, default_velocity_softcap),
          velocity_smoothing_ms: optional_u32(ed.velocity_smoothing_ms, default_velocity_smoothing_ms),
        }
      }
    })
    .collect();

  let port_rx = normalize_port_rx_for_editor(&state.port_rx);
  if port_rx.eq_ignore_ascii_case("OSCQuery") {
    cfg.setup.port_rx = "OSCQuery".to_string();
  } else if port_rx.parse::<u16>().is_err() {
    return Err(format!(
      "Invalid OSC listen port '{}'. Use a port number or OSCQuery.",
      port_rx
    ));
  } else {
    cfg.setup.port_rx = port_rx;
  }

  let yaml =
    serde_yaml::to_string(&cfg).map_err(|e| format!("Failed to serialize config: {}", e))?;
  let path = config_file_path();
  fs::write(&path, yaml).map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
  crate::device_test::invalidate_motor_cache();
  if quiet {
    let port_label = if cfg.setup.port_rx.eq_ignore_ascii_case("OSCQuery") {
      "OSCQuery".to_string()
    } else {
      format!("port {}", cfg.setup.port_rx)
    };
    log_ui::status(&format!("OSC set to {}. Reloading...", port_label));
    crate::router::request_restart_quiet();
  } else {
    log_ui::status("Configuration saved. Reloading router...");
    crate::router::request_restart();
  }
  Ok(())
}

fn name_for_yaml(name: &str) -> Option<String> {
  let name = name.trim();
  if name.is_empty() {
    None
  } else {
    Some(name.to_string())
  }
}

/// Write `{name}_online` when YAML has null/missing, or when the stored value still matches
/// the auto-generated name for the previous device name. Keep values the user set manually.
fn resolve_online_parameter_for_save(
  index: usize,
  editor_name: &str,
  previous_yaml_name: &str,
  existing: Option<&str>,
) -> String {
  let default_online = default_online_parameter_short(index, editor_name);
  let Some(short) = online_parameter_to_short(existing) else {
    return default_online;
  };
  if short.is_empty() || short.eq_ignore_ascii_case("null") {
    return default_online;
  }
  let previous_default = default_online_parameter_short(index, previous_yaml_name);
  if short == previous_default {
    default_online
  } else {
    short
  }
}

fn online_parameter_to_short(existing: Option<&str>) -> Option<String> {
  let value = existing?.trim();
  if value.is_empty() {
    return None;
  }
  const PREFIX: &str = "/avatar/parameters/";
  let short = if value.starts_with(PREFIX) {
    value[PREFIX.len()..].to_string()
  } else {
    value.trim_start_matches('/').to_string()
  };
  Some(short)
}

fn strip_avatar_parameter_short(s: &str) -> String {
  const PREFIX: &str = "/avatar/parameters/";
  let s = s.trim();
  if s.starts_with(PREFIX) {
    s[PREFIX.len()..].to_string()
  } else {
    s.trim_start_matches('/').to_string()
  }
}

fn editor_max_speed_parameter_display(yaml_value: Option<&str>) -> String {
  match yaml_value {
    Some(s) if !s.trim().is_empty() && !s.trim().eq_ignore_ascii_case("null") => {
      strip_avatar_parameter_short(s)
    }
    _ => String::new(),
  }
}

fn optional_max_speed_parameter(value: &str) -> Option<String> {
  let short = normalize_avatar_parameter_short(value);
  if short.is_empty() {
    None
  } else {
    Some(short)
  }
}

fn optional_f64(value: f32, default: f64) -> Option<f64> {
  if (f64::from(value) - default).abs() < 0.0001 {
    None
  } else {
    Some(f64::from(value))
  }
}

fn optional_u32(value: u32, default: u32) -> Option<u32> {
  if value == default {
    None
  } else {
    Some(value)
  }
}

fn normalize_avatar_parameter_short(s: &str) -> String {
  strip_avatar_parameter_short(s)
}
