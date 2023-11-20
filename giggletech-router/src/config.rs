// config.rs

use configparser::ini::Ini;
use std::{net::IpAddr};

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
                                                                                
}

#[derive(Copy, Clone)]
pub(crate) struct AdvancedConfig {
    pub active: bool,
    pub outer_proximity: f32,
    pub inner_proximity: f32,
    pub velocity_scalar: f32,
}

pub(crate) fn load_config() -> (
    Vec<String>,    // headpat_device_URIs
    f32,            // min_speed_float
    f32,            // max_speed_float
    f32,            // speed_scale_float
    String,         // port_rx
    Vec<String>,    // proximity_parameters_multi
    String,         // max_speed_parameter_address
    f32,            // Max Speed Low Limit
    u64,            // Timeout Setting
    AdvancedConfig, // Advanced mode
    ) {
    let mut config = Ini::new();

    match config.load("./config.ini") {
        Err(why) => panic!("{}", why),
        Ok(_) => {}
    }
    
    // Check the format of the IP URIs
    let headpat_device_uris: Vec<String> = config.get("Setup", "device_ips")
        .unwrap()
        .split_whitespace()
        .map(|s| s.to_string()) // convert &str to String
        .filter_map(|s| {
            match s.parse::<IpAddr>() {
                Ok(_) => Some(s),
                Err(_) => {
                    println!("Invalid IP address format: {}", s);
                    None
                }
            }
        })
        .collect();
    if headpat_device_uris.is_empty() {
        eprintln!("Error: no device URIs specified in config file");
        // handle error here, e.g. return early from the function or exit the program
    }

    let proximity_parameters_multi: Vec<String> = config
    .get("Setup", "proximity_parameters_multi")
    .unwrap()
    .split_whitespace()
    .map(|s| format!("/avatar/parameters/{}", s))
    .collect();

    
    if headpat_device_uris.len() != proximity_parameters_multi.len() {
        eprintln!("Error: number of device URIs does not match number of proximity parameters");
        // handle error here, e.g. return early from the function or exit the program
    }

    const MAX_SPEED_LOW_LIMIT_CONST: f32 = 0.05;

    let min_speed                = config.get("Config", "min_speed").unwrap();
    let min_speed_float             = min_speed.parse::<f32>().unwrap() / 100.0;
    
    let max_speed                   = config.get("Config", "max_speed").unwrap().parse::<f32>().unwrap() / 100.0; 
    let max_speed_low_limit         = MAX_SPEED_LOW_LIMIT_CONST;
    let max_speed_float             = max_speed.max(max_speed_low_limit);
    
    let speed_scale              = config.get("Config", "max_speed_scale").unwrap();
    let speed_scale_float           = speed_scale.parse::<f32>().unwrap() / 100.0;
    
    let port_rx                  = config.get("Setup", "port_rx").unwrap();
    
    let timeout_str              = config.get("Config", "timeout").unwrap();
    let timeout                     = timeout_str.parse::<u64>().unwrap_or(0);
    
    let max_speed_parameter_address = format!("/avatar/parameters/{}", config.get("Setup", "max_speed_parameter").unwrap_or_else(|| "/avatar/parameters/max_speed".into()));


    let advanced_config = load_advanced_config(config);

    println!("\n");
    banner_txt();
    println!("\n");
    println!(" Device Maps");
    for (i, parameter) in proximity_parameters_multi.iter().enumerate() {
        println!(" {} => {}", parameter.trim_start_matches("/avatar/parameters/"), headpat_device_uris[i]);
    }

    println!("\n Listening for OSC on port: {}", port_rx);
    println!("\n Vibration Configuration");
    println!(" Min Speed: {}%", min_speed);
    println!(" Max Speed: {:?}%", max_speed_float * 100.0);
    println!(" Scale Factor: {}%", speed_scale);
    println!(" Timeout: {}s", timeout);
    println!(" Advanced Mode: {}", advanced_config.active);
    println!("\nWaiting for pats...");

    (
        headpat_device_uris,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameters_multi,
        max_speed_parameter_address,
        max_speed_low_limit,
        timeout,
        advanced_config,
    )
}

pub(crate) fn load_advanced_config(config: Ini) -> AdvancedConfig {
    // println!("{}", config.get("Setup", "advanced_mode").unwrap());
    if !config.get("Setup", "advanced_mode").unwrap().eq("true") {
        return AdvancedConfig {
            active: false,
            outer_proximity: 0.0,
            inner_proximity: 0.0,
            velocity_scalar: 0.0,
        }
    }

    let outer_proximity     = config.get("Advanced", "outer_proximity").unwrap().parse::<f32>().unwrap();
    let inner_proximity     = config.get("Advanced", "inner_proximity").unwrap().parse::<f32>().unwrap();
    let velocity_scalar     = config.get("Advanced", "velocity_scalar").unwrap().parse::<f32>().unwrap();

    return AdvancedConfig {
        active: true,
        outer_proximity: outer_proximity,
        inner_proximity: inner_proximity,
        velocity_scalar: velocity_scalar,
    }
}

