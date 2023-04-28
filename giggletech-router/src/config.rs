use configparser::ini::Ini;
use std::net::IpAddr;

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

pub(crate) fn load_config() -> (
    String, // headpat_device_ip
    Vec<String>, // headpat_device_URIs
    f32,    // min_speed_float
    f32,    // max_speed_float
    f32,    // speed_scale_float
    String, // port_rx
    String, // proximity_parameter_address
    Vec<String>, // proximity_parameters_multi
    String, // max_speed_parameter_address
    f32,    // Max Speed Low Limit
    ) {
    let mut config = Ini::new();

    match config.load("./config.ini") {
        Err(why) => panic!("{}", why),
        Ok(_) => {}
    }
    const MAX_SPEED_LOW_LIMIT_CONST: f32 = 0.05;



    // Check the format of the IP URIs
    let headpat_device_uris: Vec<String> = config.get("Setup", "device_uris")
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

    println!("Device URIs: {:?}", headpat_device_uris);

    // Multi Device


    let proximity_parameters_multi: Vec<String> = config.get("Setup", "proximity_parameters_multi")
    .unwrap()
    .split_whitespace()
    .map(|s| format!("/avatar/parameters/{}", s)) // add "/avatar/parameters/" prefix to each string
    .collect();

    println!("Device URIs: {:?}", proximity_parameters_multi);

    
    if headpat_device_uris.len() != proximity_parameters_multi.len() {
        eprintln!("Error: number of device URIs does not match number of proximity parameters");
        // handle error here, e.g. return early from the function or exit the program
    }














    let headpat_device_ip   = config.get("Setup", "device_ip").unwrap();
    let headpat_device_port = "8888".to_string();
    let min_speed           = config.get("Haptic_Config", "min_speed").unwrap();
    let min_speed_float     = min_speed.parse::<f32>().unwrap() / 100.0;
    let max_speed           = config.get("Haptic_Config", "max_speed").unwrap();
    let max_speed_float     = max_speed.parse::<f32>().unwrap() / 100.0; 
    let max_speed_low_limit = MAX_SPEED_LOW_LIMIT_CONST;
    let max_speed_float     = max_speed_float.max(max_speed_low_limit);
    let speed_scale         = config.get("Haptic_Config", "max_speed_scale").unwrap();
    let speed_scale_float   = speed_scale.parse::<f32>().unwrap() / 100.0;
    let port_rx             = config.get("Setup", "port_rx").unwrap();

    let proximity_parameter_address = config
        .get("Setup", "proximity_parameter")
        .unwrap_or_else(|| "/avatar/parameters/proximity_01".into());
    let max_speed_parameter_address = config
        .get("Setup", "max_speed_parameter")
        .unwrap_or_else(|| "/avatar/parameters/max_speed".into());

    println!("\n");
    banner_txt();
    println!("\n");
    println!(" Haptic Device: {}:{}", headpat_device_ip, headpat_device_port);
    println!(" Listening for OSC on port: {}", port_rx);
    println!("\n Vibration Configuration");
    println!(" Min Speed: {}%", min_speed);
    println!(" Max Speed: {:?}%", max_speed_float * 100.0);
    println!(" Scale Factor: {}%", speed_scale);
    println!("\nWaiting for pats...");

    (
        headpat_device_ip,
        headpat_device_uris,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameter_address,
        proximity_parameters_multi,
        max_speed_parameter_address,
        max_speed_low_limit,
    )
}