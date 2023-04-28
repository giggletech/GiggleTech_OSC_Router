use configparser::ini::Ini;

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
    String, // headpat_device_port
    f32,    // min_speed_float
    f32,    // max_speed_float
    f32,    // speed_scale_float
    String, // port_rx
    String, // proximity_parameter_address
    String, // max_speed_parameter_address
    f32,    // Max Speed Low Limit
    ) {
    let mut config = Ini::new();

    match config.load("./config.ini") {
        Err(why) => panic!("{}", why),
        Ok(_) => {}
    }
    const MAX_SPEED_LOW_LIMIT_CONST: f32 = 0.05;

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
        headpat_device_port,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        max_speed_low_limit,
    )
}