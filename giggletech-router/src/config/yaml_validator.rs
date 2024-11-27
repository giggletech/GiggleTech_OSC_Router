use serde::Deserialize;
use serde_yaml::{self, Error};
use std::fs;

// Define the structure of your YAML configuration
#[derive(Debug, Deserialize)]
pub struct Setup {
    pub port_rx: String,
    pub default_min_speed: Option<u32>,
    pub default_max_speed: Option<u32>,
    pub default_start_tx: Option<u32>,
    pub default_max_speed_parameter: Option<String>,
    pub timeout: Option<u32>,
    pub default_use_velocity_control: Option<bool>,
    pub default_outer_proximity: Option<f64>,
    pub default_inner_proximity: Option<f64>,
    pub default_velocity_scalar: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Device {
    pub ip: String,
    pub proximity_parameter: String,
    pub max_speed: Option<u32>,
    pub speed_scale: Option<u32>,
    pub max_speed_parameter: Option<String>,
    pub use_velocity_control: Option<bool>,
    pub outer_proximity: Option<f64>,
    pub inner_proximity: Option<f64>,
    pub velocity_scalar: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub devices: Vec<Device>,
    pub setup: Setup,
}

/// Validates a YAML file against the `Config` structure.
///
/// # Arguments
/// * `file_path` - Path to the YAML file.
///
/// # Returns
/// * `Ok(())` if the YAML is valid.
/// * `Err(String)` with a description of the error if invalid.
pub fn validate_yaml(file_path: &str) -> Result<(), String> {
    let file_content = fs::read_to_string(file_path)
        .map_err(|e| format!("Error reading file: {}", e))?;

    // Parse the YAML into a generic Value for manual validation
    let raw_yaml: serde_yaml::Value = serde_yaml::from_str(&file_content)
        .map_err(|e| {
            if let Some(loc) = e.location() {
                let lines: Vec<&str> = file_content.lines().collect();
                let error_line = lines.get(loc.line() - 1).unwrap_or(&"<unable to retrieve line>");
                let previous_line = lines.get(loc.line().saturating_sub(2)).unwrap_or(&"<no previous line>");
                format!(
                    "YAML Validation Error: Missing ':' or key-value separator at line {}.\n> {}\n> {}\nNote: Check for errors above these lines.",
                    loc.line(),
                    previous_line.trim(),
                    error_line.trim()
                )
            } else {
                format!("YAML Validation Error: {}", e)
            }
        })?;

    // Validate the `setup` section
    if let Some(setup) = raw_yaml.get("setup").and_then(|s| s.as_mapping()) {
        // Check for required fields in `setup`
        if !setup.contains_key(&serde_yaml::Value::String("port_rx".to_string())) {
            return Err("YAML Validation Error: Missing 'port_rx' field in 'setup' section.".to_string());
        }
        if !setup.contains_key(&serde_yaml::Value::String("default_min_speed".to_string())) {
            return Err("YAML Validation Error: Missing 'default_min_speed' field in 'setup' section.".to_string());
        }
        if !setup.contains_key(&serde_yaml::Value::String("default_max_speed".to_string())) {
            return Err("YAML Validation Error: Missing 'default_max_speed' field in 'setup' section.".to_string());
        }
        // Add more checks as needed for the setup fields
    } else {
        return Err("YAML Validation Error: Missing or invalid 'setup' section.".to_string());
    }

    // If parsing and setup validation succeed, continue
    Ok(())
}
