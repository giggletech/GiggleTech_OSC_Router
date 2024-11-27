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

    // Attempt to parse the YAML file
    let config: Result<Config, Error> = serde_yaml::from_str(&file_content);

    match config {
        Ok(_) => Ok(()),
        Err(e) => {
            // Extract error location
            let location = e.location();
            if let Some(loc) = location {
                let lines: Vec<&str> = file_content.lines().collect();

                // Find surrounding lines for context
                let error_line = lines.get(loc.line() - 1).unwrap_or(&"<unable to retrieve line>");
                let prev_line = lines.get(loc.line().saturating_sub(2)).unwrap_or(&"");

                // Check for possible syntax issues in the line and surrounding context
                let suspected_issue = if !error_line.contains(':') && error_line.trim().contains(' ') {
                    "Possible missing colon ':' detected in this line or the line above "
                } else {
                    ""
                };

                Err(format!(
                    "YAML Validation Error at line {} column {}: {}\n> {}\n{}",
                    loc.line(),
                    loc.column(),
                    e,
                    error_line.trim(),
                    suspected_issue
                ))
            } else {
                // Fallback if no location is available
                Err(format!("YAML Validation Error: {}", e))
            }
        }
    }
}