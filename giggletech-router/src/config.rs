// config.rs

use std::{net::IpAddr};
use std::fs::File;
use std::io::Read;
use std::sync::Arc;
use yaml_rust::{YamlLoader, Yaml};
use yaml_rust::yaml::Hash;

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
    println!(" v1.2");
                                                                                
}

#[derive(Clone, Debug)]
pub(crate) struct DeviceConfig {
    pub device_uri: Arc<String>,
    pub min_speed: f32,
    pub max_speed: f32,
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

pub(crate) fn load_config() -> (GlobalConfig, Vec<DeviceConfig>) {
    let mut config_file = match File::open("./config.yml") {
        Err(why) => panic!("{}", why),
        Ok(f) => f
    };
    let mut config_data = String::new();
    match config_file.read_to_string(&mut config_data) {
        Err(why) => panic!("{}", why),
        // don't care how many bytes were read
        Ok(_) => {}
    }

    let config = match YamlLoader::load_from_str(&config_data) {
        Err(why) => panic!("{}", why),
        Ok(yaml_data) => yaml_data
    };
    assert_eq!(config.len(), 1, "Only 1 element should be in the yaml file");
    let map = config.first().unwrap().as_hash().expect("Expected config to be a map at the top level");
    let setup = map.get(&Yaml::String("setup".to_string())).expect("Missing setup section").as_hash().expect("Setup section must be a map");
    let setup = YamlHashWrapper {yaml_hash: setup.clone()};
    let global_config = parse_global_config(setup);

    let devices = map.get(&Yaml::String("devices".to_string())).expect("Missing devices section").as_vec().expect("Devices section must be a list");
    let devices: Vec<DeviceConfig> = devices.iter().map(|dev| {
        let device_data = YamlHashWrapper {yaml_hash: dev.as_hash().unwrap().clone()};
        parse_device_config(device_data, &global_config)
    }).collect();

    println!("\n");
    banner_txt();
    println!("\n");
    println!(" Device Maps");
    for (i, device) in devices.iter().enumerate() {
        println!("  Device {i}");
        println!("   {} => {}", device.proximity_parameter.trim_start_matches("/avatar/parameters/"), device.device_uri);
        println!("   Vibration Configuration");
        println!("    Min Speed: {:?}%", device.min_speed * 100.0);
        println!("    Max Speed: {:?}%", device.max_speed * 100.0);
        println!("    Scale Factor: {:?}%", device.speed_scale * 100.0);
        println!("    Advanced Mode: {}", device.use_velocity_control);
    }

    println!("\n Listening for OSC on port: {}", global_config.port_rx);
    println!(" Timeout: {}s", global_config.timeout);
    println!("\nWaiting for pats...");

    (global_config, devices)
}

fn parse_global_config(setup: YamlHashWrapper) -> GlobalConfig {
    let port_rx = Arc::new(setup.get_str("port_rx").unwrap());
    // only allow valid ports
    assert!(u16::from_str_radix(&port_rx, 10).is_ok());

    let default_min_speed = setup.get_f64("default_min_speed").unwrap() as f32 / 100.0;
    // negative speeds don't make sense
    assert!(default_min_speed >= 0.0);

    const MAX_SPEED_LOW_LIMIT_CONST: f32 = 0.05;

    let default_max_speed = setup.get_f64("default_max_speed").unwrap() as f32 / 100.0;
    let default_max_speed = default_max_speed.max(default_min_speed).max(MAX_SPEED_LOW_LIMIT_CONST);

    let default_max_speed_parameter = setup.get_str("default_max_speed_parameter").unwrap_or("max_speed".to_string());
    let default_max_speed_parameter = Arc::new(format!("/avatar/parameters/{}", default_max_speed_parameter));

    let default_speed_scale = setup.get_f64("default_speed_scale").unwrap() as f32 / 100.0;

    let timeout = setup.get_i64("timeout").unwrap_or(0) as u64;

    let default_use_velocity_control = setup.get_bool("default_use_velocity_control").unwrap();
    let default_outer_proximity = setup.get_f64("default_outer_proximity").unwrap() as f32;
    let default_inner_proximity = setup.get_f64("default_inner_proximity").unwrap() as f32;
    let default_velocity_scalar = setup.get_f64("default_velocity_scalar").unwrap() as f32;

    GlobalConfig {
        port_rx,
        default_min_speed,
        default_max_speed,
        default_max_speed_parameter,
        minimum_max_speed: MAX_SPEED_LOW_LIMIT_CONST,
        default_speed_scale,
        timeout,
        default_use_velocity_control,
        default_outer_proximity,
        default_inner_proximity,
        default_velocity_scalar
    }
}

fn parse_device_config(device_data: YamlHashWrapper, global_config: &GlobalConfig) -> DeviceConfig {
    let ip = Arc::new(device_data.get_str("ip").unwrap());
    ip.as_str().parse::<IpAddr>().expect(&format!("Invalid IP address format: {}", ip));
    let proximity_parameter = Arc::new(format!("/avatar/parameters/{}", device_data.get_str("proximity_parameter").unwrap()));
    let min_speed = device_data.get_f64("min_speed").map(|x| x as f32 / 100.0).unwrap_or(global_config.default_min_speed);
    assert!(min_speed >= 0.0);
    let max_speed = device_data.get_f64("max_speed").map(|x| (x as f32 / 100.0).max(min_speed).max(global_config.minimum_max_speed)).unwrap_or(global_config.default_max_speed);
    let speed_scale = device_data.get_f64("speed_scale").map(|x| x as f32 / 100.0).unwrap_or(global_config.default_speed_scale);
    let max_speed_parameter = device_data.get_str("max_speed_parameter").map(|x| Arc::new(format!("/avatar/parameters/{}", x))).unwrap_or(global_config.default_max_speed_parameter.clone());
    let use_velocity_control = device_data.get_bool("use_velocity_control").unwrap_or(global_config.default_use_velocity_control);
    let outer_proximity = device_data.get_f64("outer_proximity").map(|x| x as f32).unwrap_or(global_config.default_outer_proximity);
    let inner_proximity = device_data.get_f64("inner_proximity").map(|x| x as f32).unwrap_or(global_config.default_inner_proximity);
    let velocity_scalar = device_data.get_f64("velocity_scalar").map(|x| x as f32).unwrap_or(global_config.default_velocity_scalar);

    DeviceConfig {
        device_uri: ip,
        proximity_parameter,
        min_speed,
        max_speed,
        speed_scale,
        max_speed_parameter,
        use_velocity_control,
        outer_proximity,
        inner_proximity,
        velocity_scalar
    }
}