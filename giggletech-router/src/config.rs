/*
    config.rs - Configuration Module for Giggletech VRChat OSC System

    This module is responsible for loading, parsing, and managing the configuration settings 
    for the VRChat OSC-based system. It reads configuration from a `config.yml` file, processes 
    both global and device-specific settings, and manages important parameters like OSC ports, 
    speed, proximity, and velocity control. It also supports dynamic port retrieval via OSCQuery.

    **Key Features:**
    
    1. **Loading Configuration (`load_config`)**:
       - Reads the `config.yml` file and parses it into a structure using YAML.
       - Extracts global and device-specific settings.
       - Displays a banner with device information and listens for OSC messages on a defined port.

    2. **Global Configuration (`GlobalConfig`)**:
       - The global settings include defaults for min/max speeds, proximity parameters, and OSC ports.
       - The function `parse_global_config` handles both static OSC ports and dynamic ports through OSCQuery.
       - Key parameters include:
         - `port_rx`: The OSC port (either a fixed value or dynamically assigned via OSCQuery).
         - `default_min_speed` & `default_max_speed`: Speed limits used for device control.
         - `timeout`, `velocity control`, and `proximity settings`.

    3. **Device-Specific Configuration (`DeviceConfig`)**:
       - Each device can have custom parameters, but if not specified, they inherit from global settings.
       - The function `parse_device_config` processes each device's configuration, allowing custom IP addresses, 
         speed settings, and proximity parameters for each individual device.

    **Dynamic Port Management with OSCQuery**:
    - If the configuration specifies `"OSCQuery"` for `port_rx`, the module uses the `oscq_giggletech` helper 
      to dynamically retrieve the UDP port from the OSCQuery service. If not, a static port number from the config is used.

    **Usage**:
    - After parsing the configuration, the module initializes the devices and starts listening for OSC messages 
      on the specified port. It supports multiple devices, each with their unique or global configurations.
    
    **Example Configurations**:
    ```yaml
    setup:
      port_rx: OSCQuery  # Uses dynamic OSCQuery port
      default_min_speed: 0.1
      default_max_speed: 1.0

    devices:
      - ip: "192.168.1.2"
        min_speed: 0.2
        max_speed: 0.8
    ```
*/


// NOTE REMOVED  from YML still here, not really used # Maximum Speed Scalar (10-100)
//  #default_speed_scale: 100

use std::{net::IpAddr};
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use yaml_rust::{YamlLoader, Yaml};
use yaml_rust::yaml::Hash;
mod oscq_giggletech;

use std::fs::OpenOptions;
use std::io::Write;
use chrono::Local; // For timestamps


mod yaml_validator;

use yaml_validator::{validate_yaml, Config};


fn log_to_file(message: &str) {
    let now = Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S").to_string(); // Add a timestamp
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open("giggletech_log.txt")
        .unwrap();
    writeln!(file, "[{}] {}", timestamp, message).unwrap();
}

// Banner
fn banner_txt(){
    // https://fsymbols.com/generators/carty/
    println!("");
    println!("  ██████  ██  ██████   ██████  ██      ███████     ████████ ███████  ██████ ██   ██ ");
    println!(" ██       ██ ██       ██       ██      ██             ██    ██      ██      ██   ██ ");
    println!(" ██   ███ ██ ██   ███ ██   ███ ██      █████          ██    █████   ██      ███████ ");
    println!(" ██    ██ ██ ██    ██ ██    ██ ██      ██             ██    ██      ██      ██   ██ ");
    println!("  ██████  ██  ██████   ██████  ███████ ███████        ██    ███████  ██████ ██   ██ ");
    println!("");
    println!(" █▀█ █▀ █▀▀   █▀█ █▀█ █ █ ▀█▀ █▀▀ █▀█");
    println!(" █▄█ ▄█ █▄▄   █▀▄ █▄█ █▄█  █  ██▄ █▀▄");
    println!("");
    println!(" v1.3.2");
                                                                                
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceConfig {
    pub device_uri: Arc<String>,
    pub min_speed: f32,
    pub max_speed: f32,
    pub start_tx: i32,
    pub speed_scale: f32,
    pub proximity_parameter: Arc<String>,
    pub max_speed_parameter: Arc<String>,
    pub use_velocity_control: bool,
    pub outer_proximity: f32,
    pub inner_proximity: f32,
    pub velocity_scalar: f32
}

#[derive(Clone, Debug)]
pub(crate) struct GlobalConfig {
    pub port_rx: Arc<String>,
    pub default_min_speed: f32,
    pub default_max_speed: f32,
    pub default_speed_scale: f32,
    pub default_start_tx: i32,
    pub default_max_speed_parameter: Arc<String>,
    pub minimum_max_speed: f32,
    pub timeout: u64,
    pub default_use_velocity_control: bool,
    pub default_outer_proximity: f32,
    pub default_inner_proximity: f32,
    pub default_velocity_scalar: f32
}

struct YamlHashWrapper {
    yaml_hash: Hash
}

impl YamlHashWrapper {
    fn has_key(&self, key: &str) -> bool {
        self.yaml_hash.contains_key(&Yaml::String(key.to_string()))
    }

    fn get_i64(&self, key: &str) -> Option<i64> {
        self.yaml_hash.get(&Yaml::String(key.to_string()))?.as_i64()
    }

    fn get_f64(&self, key: &str) -> Option<f64> {
        let value = self.yaml_hash.get(&Yaml::String(key.to_string()));
        value.map(|yaml| {
            yaml.as_f64()
                .or(yaml.as_i64().map(|x| x as f64))
        }).flatten()
    }

    fn get_str(&self, key: &str) -> Option<String> {
        let value = self.yaml_hash.get(&Yaml::String(key.to_string()));
        value.map(|yaml| {
            yaml.as_str().map(|x| x.to_string())
                .or(yaml.as_bool().map(|x| x.to_string()))
                .or(yaml.as_i64().map(|x| x.to_string()))
                .or(yaml.as_f64().map(|x| x.to_string()))
        }).flatten()
    }

    fn get_bool(&self, key: &str) -> Option<bool> {
        self.yaml_hash.get(&Yaml::String(key.to_string()))?.as_bool()
    }
}









pub(crate) fn load_config() -> Result<(GlobalConfig, Vec<DeviceConfig>), String> {
    let mut config_file = match File::open("./config.yml") {
        Err(why) => return Err(format!("Failed to open config.yml: {}", why)),
        Ok(f) => f
    };

    // Call validate_yaml function
    match validate_yaml("./config.yml") {
        Ok(_) => println!("Configuration file is valid."),
        Err(e) => return Err(format!("Configuration File Error: {}", e)),
    };

    let mut config_data = String::new();
    match config_file.read_to_string(&mut config_data) {
        Err(why) => return Err(format!("Failed to read config.yml: {}", why)),
        Ok(_) => {}
    }

    let config = match YamlLoader::load_from_str(&config_data) {
        Err(why) => return Err(format!("Failed to parse YAML: {}", why)),
        Ok(yaml_data) => yaml_data
    };
    
    if config.len() != 1 {
        return Err("Only 1 element should be in the yaml file".to_string());
    }
    
    let map = match config.first().unwrap().as_hash() {
        Some(hash) => hash,
        None => return Err("Expected config to be a map at the top level".to_string()),
    };
    
    let setup = match map.get(&Yaml::String("setup".to_string())) {
        Some(setup_yaml) => match setup_yaml.as_hash() {
            Some(setup_hash) => setup_hash,
            None => return Err("Setup section must be a map".to_string()),
        },
        None => return Err("Missing setup section".to_string()),
    };
    
    let setup = YamlHashWrapper {yaml_hash: setup.clone()};
    let global_config = parse_global_config(setup);

    let devices = match map.get(&Yaml::String("devices".to_string())) {
        Some(devices_yaml) => match devices_yaml.as_vec() {
            Some(devices_vec) => devices_vec,
            None => return Err("Devices section must be a list".to_string()),
        },
        None => return Err("Missing devices section".to_string()),
    };
    
    let mut device_configs = Vec::new();
    for (i, dev) in devices.iter().enumerate() {
        let device_hash = match dev.as_hash() {
            Some(hash) => hash,
            None => return Err(format!("Device {} is not a valid map", i + 1)),
        };
        let device_data = YamlHashWrapper {yaml_hash: device_hash.clone()};
        match parse_device_config(device_data, &global_config) {
            Ok(device_config) => device_configs.push(device_config),
            Err(e) => return Err(format!("Error parsing device {}: {}", i + 1, e)),
        }
    }

    println!("\n");
    banner_txt();
    println!("\n");
    println!(" Device Maps");
    println!("");
    for (i, device) in device_configs.iter().enumerate() {
        println!("  Device {i}");
        println!("   {} => {}", device.proximity_parameter.trim_start_matches("/avatar/parameters/"), device.device_uri);
        println!("   Vibration Configuration");
        println!("    Startup TX Speed: {:.0}%", device.start_tx);
        println!("    Min Speed: {:.0}%", device.min_speed * 100.0);
        println!("    Max Speed: {:.0}%", device.max_speed * 100.0);
        println!("    Scale Factor: {:.0}%", device.speed_scale * 100.0);
        println!("    Advanced Mode: {}", device.use_velocity_control);
        println!("");
    }

    println!("\n Listening for OSC on port: {}", global_config.port_rx);
    println!(" Timeout: {}s", global_config.timeout);
    println!("\nWaiting for pats...");

    Ok((global_config, device_configs))
}



fn parse_global_config(setup: YamlHashWrapper) -> GlobalConfig {
    // Retrieve the value of `port_rx` from the YAML file with fallback
    let port_rx_str = setup.get_str("port_rx").unwrap_or_else(|| {
        println!("Warning: port_rx not found in config, using default port 9001");
        "9001".to_string()
    });

    // Check if `port_rx` is "OSCQuery" or a numeric port
    let port_rx: Arc<String> = if port_rx_str == "OSCQuery" {
        // If it's "OSCQuery", try to use the port from the OSCQuery server
        println!("\nAttempting to use OSCQuery...");
        match std::panic::catch_unwind(|| {
            oscq_giggletech::initialize_and_get_udp_port()
        }) {
            Ok(udp_port) => {
                println!("OSCQuery initialized successfully. UDP port: {}", udp_port);
                Arc::new(udp_port.to_string())
            }
            Err(_) => {
                println!("OSCQuery initialization failed. Falling back to default port 9001.");
                Arc::new("9001".to_string())
            }
        }
    } else {
        // Otherwise, assume it's a port number in string format, validate, and wrap it in Arc
        match u16::from_str_radix(&port_rx_str, 10) {
            Ok(_) => Arc::new(port_rx_str),
            Err(_) => {
                println!("Warning: Invalid port number '{}', using default port 9001", port_rx_str);
                Arc::new("9001".to_string())
            }
        }
    };

    // Proceed with other config values with proper fallbacks
    let default_min_speed = setup.get_f64("default_min_speed").unwrap_or(5.0) as f32 / 100.0;
    assert!(default_min_speed >= 0.0); // Ensure min speed is valid

    const MAX_SPEED_LOW_LIMIT_CONST: f32 = 0.05;

    let default_max_speed = setup.get_f64("default_max_speed").unwrap_or(25.0) as f32 / 100.0;
    let default_max_speed = default_max_speed.max(default_min_speed).max(MAX_SPEED_LOW_LIMIT_CONST);

    let default_start_tx = setup.get_i64("default_start_tx").unwrap_or(20) as i32;

    let default_max_speed_parameter = setup
        .get_str("default_max_speed_parameter")
        .unwrap_or("max_speed".to_string());
    let default_max_speed_parameter = Arc::new(format!("/avatar/parameters/{}", default_max_speed_parameter));

    let default_speed_scale = (setup.get_f64("default_speed_scale").unwrap_or(100.0) as f32) / 100.0;

    let timeout = setup.get_i64("timeout").unwrap_or(5) as u64;

    let default_use_velocity_control = setup
        .get_bool("default_use_velocity_control")
        .or_else(|| {
            setup
                .get_str("default_use_velocity_control")
                .map(|s| s.to_lowercase() == "true")
        })
        .unwrap_or(false); // Default to `false` if the key is missing or invalid

    let default_outer_proximity = setup.get_f64("default_outer_proximity").unwrap_or(0.0) as f32;
    let default_inner_proximity = setup.get_f64("default_inner_proximity").unwrap_or(0.7) as f32;
    let default_velocity_scalar = setup.get_f64("default_velocity_scalar").unwrap_or(20.0) as f32;

    // Return the GlobalConfig struct with the updated port_rx
    GlobalConfig {
        port_rx,
        default_min_speed,
        default_max_speed,
        default_max_speed_parameter,
        default_start_tx,
        minimum_max_speed: MAX_SPEED_LOW_LIMIT_CONST,
        default_speed_scale,
        timeout,
        default_use_velocity_control,
        default_outer_proximity,
        default_inner_proximity,
        default_velocity_scalar,
    }
}


fn parse_device_config(device_data: YamlHashWrapper, global_config: &GlobalConfig) -> Result<DeviceConfig, String> {
    let ip = match device_data.get_str("ip") {
        Some(ip_str) => {
            match ip_str.parse::<IpAddr>() {
                Ok(_) => Arc::new(ip_str),
                Err(_) => {
                    return Err(format!("Invalid IP address format: {}", ip_str));
                }
            }
        }
        None => {
            return Err("Missing 'ip' field in device configuration".to_string());
        }
    };

    let proximity_parameter = match device_data.get_str("proximity_parameter") {
        Some(param) => Arc::new(format!("/avatar/parameters/{}", param)),
        None => {
            return Err("Missing 'proximity_parameter' field in device configuration".to_string());
        }
    };

    let min_speed = device_data.get_f64("min_speed").map(|x| x as f32 / 100.0).unwrap_or(global_config.default_min_speed);
    if min_speed < 0.0 {
        return Err("Min speed cannot be negative".to_string());
    }
    
    let max_speed = device_data.get_f64("max_speed").map(|x| (x as f32 / 100.0).max(min_speed).max(global_config.minimum_max_speed)).unwrap_or(global_config.default_max_speed);
    let start_tx = device_data.get_i64("start_tx").map(|x| x as i32).unwrap_or(global_config.default_start_tx);
    let speed_scale = device_data.get_f64("speed_scale").map(|x| x as f32 / 100.0).unwrap_or(global_config.default_speed_scale);
    let max_speed_parameter = device_data.get_str("max_speed_parameter").map(|x| Arc::new(format!("/avatar/parameters/{}", x))).unwrap_or(global_config.default_max_speed_parameter.clone());
    let use_velocity_control = device_data.get_bool("use_velocity_control").unwrap_or(global_config.default_use_velocity_control);
    let outer_proximity = device_data.get_f64("outer_proximity").map(|x| x as f32).unwrap_or(global_config.default_outer_proximity);
    let inner_proximity = device_data.get_f64("inner_proximity").map(|x| x as f32).unwrap_or(global_config.default_inner_proximity);
    let velocity_scalar = device_data.get_f64("velocity_scalar").map(|x| x as f32).unwrap_or(global_config.default_velocity_scalar);

    // Log device settings
    log_to_file(&format!(
        "Device IP: {}\nMin Speed: {:.0}%\nMax Speed: {:.0}%\nSpeed Scale: {:.0}%\nProximity Parameter: {}\nVelocity Control: {}\nOuter Proximity: {:.2}\nInner Proximity: {:.2}\n",
        ip, min_speed * 100.0, max_speed * 100.0, speed_scale * 100.0, proximity_parameter, use_velocity_control, outer_proximity, inner_proximity
    ));

    Ok(DeviceConfig {
        device_uri: ip,
        proximity_parameter,
        min_speed,
        max_speed,
        start_tx,
        speed_scale,
        max_speed_parameter,
        use_velocity_control,
        outer_proximity,
        inner_proximity,
        velocity_scalar
    })
}
