use serde::{Deserialize, Serialize};
use serde_yaml;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub devices: Vec<Device>,
    pub setup: Setup,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Device {
    pub ip: String,
    pub proximity_parameter: String,
    #[serde(default)]
    pub max_speed: Option<u32>,
    #[serde(default)]
    pub speed_scale: Option<u32>,
    #[serde(default)]
    pub max_speed_parameter: Option<String>,
    #[serde(default)]
    pub use_velocity_control: Option<bool>,
    #[serde(default)]
    pub outer_proximity: Option<f64>,
    #[serde(default)]
    pub inner_proximity: Option<f64>,
    #[serde(default)]
    pub velocity_scalar: Option<u32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Setup {
    pub port_rx: String,
    pub default_min_speed: u32,
    pub default_max_speed: u32,
    pub default_start_tx: u32,
    pub default_max_speed_parameter: String,
    pub timeout: u32,
    pub default_use_velocity_control: bool,
    pub default_outer_proximity: f64,
    pub default_inner_proximity: f64,
    pub default_velocity_scalar: u32,
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
