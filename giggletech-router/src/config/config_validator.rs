use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub devices: Vec<Device>,
    pub setup: Setup,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(from = "DeviceYaml")]
pub struct Device {
    #[serde(default)]
    pub name: Option<String>,
    pub ip: String,
    pub proximity_parameter: String,
    #[serde(default)]
    pub online_parameter: Option<String>,
    #[serde(default)]
    pub max_speed: Option<u32>,
    #[serde(default)]
    pub speed_scale: Option<u32>,
    #[serde(default)]
    pub max_speed_parameter: Option<String>,
    #[serde(default)]
    pub use_velocity_control: Option<bool>,
    #[serde(default)]
    pub velocity_on_prox_drop: Option<bool>,
    #[serde(default)]
    pub outer_proximity: Option<f64>,
    #[serde(default)]
    pub inner_proximity: Option<f64>,
    #[serde(default)]
    pub velocity_scalar: Option<u32>,
    #[serde(default)]
    pub velocity_softcap: Option<u32>,
    #[serde(default)]
    pub velocity_smoothing_ms: Option<u32>,
}

/// Raw device row: accepts legacy YAML where `max_speed` was a VRChat parameter name string.
#[derive(Deserialize)]
struct DeviceYaml {
    #[serde(default)]
    name: Option<String>,
    ip: String,
    proximity_parameter: String,
    #[serde(default)]
    online_parameter: Option<String>,
    #[serde(default)]
    max_speed: Option<serde_yaml::Value>,
    #[serde(default)]
    speed_scale: Option<serde_yaml::Value>,
    #[serde(default)]
    max_speed_parameter: Option<String>,
    #[serde(default)]
    use_velocity_control: Option<bool>,
    #[serde(default)]
    velocity_on_prox_drop: Option<bool>,
    #[serde(default)]
    outer_proximity: Option<f64>,
    #[serde(default)]
    inner_proximity: Option<f64>,
    #[serde(default)]
    velocity_scalar: Option<u32>,
    #[serde(default)]
    velocity_softcap: Option<u32>,
    #[serde(default)]
    velocity_smoothing_ms: Option<u32>,
}

impl From<DeviceYaml> for Device {
    fn from(raw: DeviceYaml) -> Self {
        let (max_speed, max_speed_parameter) =
            parse_max_speed_and_parameter(raw.max_speed, raw.max_speed_parameter);
        Device {
            name: raw.name,
            ip: raw.ip,
            proximity_parameter: raw.proximity_parameter,
            online_parameter: raw.online_parameter,
            max_speed,
            speed_scale: optional_u32_from_value(raw.speed_scale),
            max_speed_parameter,
            use_velocity_control: raw.use_velocity_control,
            velocity_on_prox_drop: raw.velocity_on_prox_drop,
            outer_proximity: raw.outer_proximity,
            inner_proximity: raw.inner_proximity,
            velocity_scalar: raw.velocity_scalar,
            velocity_softcap: raw.velocity_softcap,
            velocity_smoothing_ms: raw.velocity_smoothing_ms,
        }
    }
}

fn optional_u32_from_value(value: Option<serde_yaml::Value>) -> Option<u32> {
    match value {
        None | Some(serde_yaml::Value::Null) => None,
        Some(serde_yaml::Value::Number(n)) => n.as_u64().map(|x| x as u32),
        Some(serde_yaml::Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

/// Legacy configs used `max_speed: max_speed` (parameter name) instead of a percent.
fn parse_max_speed_and_parameter(
    max_speed: Option<serde_yaml::Value>,
    max_speed_parameter: Option<String>,
) -> (Option<u32>, Option<String>) {
    match max_speed {
        None | Some(serde_yaml::Value::Null) => (None, max_speed_parameter),
        Some(serde_yaml::Value::Number(n)) => (n.as_u64().map(|x| x as u32), max_speed_parameter),
        Some(serde_yaml::Value::String(s)) => {
            let param = max_speed_parameter.or(Some(s));
            (None, param)
        }
        _ => (None, max_speed_parameter),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Setup {
    pub port_rx: String,
    pub default_min_speed: u32,
    pub default_max_speed: u32,
    pub default_start_tx: u32,
    pub default_max_speed_parameter: String,
    pub timeout: u32,
    pub default_use_velocity_control: bool,
    #[serde(default)]
    pub default_velocity_on_prox_drop: bool,
    pub default_outer_proximity: f64,
    pub default_inner_proximity: f64,
    pub default_velocity_scalar: u32,
    #[serde(default = "default_velocity_softcap")]
    pub default_velocity_softcap: u32,
    #[serde(default)]
    pub default_velocity_smoothing_ms: u32,
    /// Resend online OSC every N seconds (0 = only on state change).
    #[serde(default)]
    pub online_status_broadcast_seconds: u32,
}

fn default_velocity_softcap() -> u32 {
    35
}

/// Reads and parses a YAML configuration file.
/// Returns a `Result` containing either the `Config` struct or an error message.
///
/// # Arguments
/// * `file_path` - Path to the YAML file.
///
/// # Example
/// ```
/// let config = yaml_parser::load_config("config.yml").unwrap();
/// ```
pub fn load_config<P: AsRef<Path>>(file_path: P) -> Result<Config, String> {
    match fs::read_to_string(file_path) {
        Ok(contents) => match serde_yaml::from_str::<Config>(&contents) {
            Ok(config) => Ok(config),
            Err(e) => Err(format!("YAML parsing error: {}", e)),
        },
        Err(e) => Err(format!("Error reading the file: {}", e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_max_speed_parameter_string() {
        let yaml = r#"
devices:
  - ip: 192.168.1.1
    proximity_parameter: proximity_01
    max_speed: max_speed
setup:
  port_rx: "9001"
  default_min_speed: 5
  default_max_speed: 25
  default_start_tx: 20
  default_max_speed_parameter: max_speed
  timeout: 5
  default_use_velocity_control: false
  default_outer_proximity: 0.0
  default_inner_proximity: 1.0
  default_velocity_scalar: 20
"#;
        let cfg: Config = serde_yaml::from_str(yaml).expect("legacy max_speed string should parse");
        assert_eq!(cfg.devices[0].max_speed, None);
        assert_eq!(
            cfg.devices[0].max_speed_parameter.as_deref(),
            Some("max_speed")
        );
    }

    #[test]
    fn numeric_max_speed_still_works() {
        let yaml = r#"
devices:
  - ip: 192.168.1.1
    proximity_parameter: proximity_01
    max_speed: 18
setup:
  port_rx: "9001"
  default_min_speed: 5
  default_max_speed: 25
  default_start_tx: 20
  default_max_speed_parameter: max_speed
  timeout: 5
  default_use_velocity_control: false
  default_outer_proximity: 0.0
  default_inner_proximity: 1.0
  default_velocity_scalar: 20
"#;
        let cfg: Config = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cfg.devices[0].max_speed, Some(18));
    }
}
