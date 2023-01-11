// Headpat IO 
// by Sideways / Jason Beattie

use async_osc::{prelude::*, OscPacket, OscSocket, OscType, Result};
use async_std::stream::StreamExt;
use configparser::ini::Ini;
use log::kv::value;

fn proximity_graph(proximity_signal: f32) -> String {
    
    let num_dashes = (proximity_signal * 10.0) as i32; // Calculate number of dashes based on scale value
    let mut graph = "".to_string(); // Initialize empty string

    graph.push_str("-".repeat(num_dashes as usize).as_str()); // Add dashes to string
    graph.push('>'); // Add arrow character to end of string

    graph // Return graph string
}

fn print_speed_limit(headpat_max_rx: f32) {

    let headpat_max_rx_print = (headpat_max_rx * 100.0).round();

    let max_meter = match headpat_max_rx_print {
        n if n > 90.0 => "!!! SO MUCH !!!",
        n if n > 75.0 => "!! ",
        n if n > 50.0 => "!  ",
        _ => "   ",
    };

    println!("Speed Limit: {}% {}", headpat_max_rx_print, max_meter);
}


fn process_pat(proximity_signal: f32, max_speed: f32, min_speed: f32, speed_scale: f32) -> i32 {

    const MOTOR_SPEED_SCALE: f32 = 0.66; // Motor is being powered off the 5v rail, rated for 3.3v, scaled arrcordingly
    let graph_str =  proximity_graph(proximity_signal); // collect graph 
    let headpat_delta:f32 = max_speed - min_speed; // Take the differance, so when at low proximetery values, the lowest value still buzzes the motor                      
    
    let headpat_tx = headpat_delta * proximity_signal + min_speed;
    let headpat_tx = headpat_tx * MOTOR_SPEED_SCALE * speed_scale* 255.0;
    
    let mut headpat_tx_int = headpat_tx as i32;
    let proximity_signal_string = format!("{:.2}", proximity_signal);
    let max_speed = format!("{:.2}", max_speed);

    // Added, as the deadzone would allow min speed to be passed through with motor stating on min speed
    if proximity_signal == 0.0{
        headpat_tx_int = 0;
    }
    eprintln!("Prox: {:5} Motor Tx: {:3}  Max Speed: {:5} |{:11}|", proximity_signal_string, headpat_tx_int, max_speed, graph_str );

    headpat_tx_int
}

fn deadzone(mut value: f32, deadzone_inner: f32, mut deadzone_outer: f32) -> f32{

    // Calculated Deadzone for optimal pat feel
    let deadzone_delta;
    if deadzone_inner <= deadzone_outer {
        println!("Incorrect Deadzone Setup!");
    }
    else{
        deadzone_delta = deadzone_inner - deadzone_outer;
        
        if value < deadzone_outer{
            value = 0.0;
        }
    
        else if value > deadzone_inner{
            value = 1.0;
        } 
    
        else{
            value = (value - deadzone_outer)*(1.0/deadzone_delta);
        }
    }
    value

}


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


fn load_config() -> (String, String, f32, f32, f32, String, String, String, f32, f32, String, String) {
    let mut config = Ini::new();

    match config.load("./config.ini") {
        Err(why) => panic!("{}", why),
        Ok(_) => {}
    }

    let headpat_device_ip = config.get("Setup", "device_ip").unwrap();
    //let headpat_device_port = config.get("Device_Setup", "headpat_io_port").unwrap(); // REMOVE ABILITY TO CHANGE PORT FROM CONFIG
    let headpat_device_port = "8888".to_string();
    let min_speed = config.get("Haptic_Config", "min_speed").unwrap();
    let min_speed_float: f32 = min_speed.parse().unwrap();
    let min_speed_float: f32 = min_speed_float / 100.0;
    
    let max_speed = config.get("Haptic_Config", "max_speed").unwrap();
    let max_speed_float: f32 = max_speed.parse().unwrap();
    let mut max_speed_float: f32 = max_speed_float / 100.0;
    const MAX_SPEED_LOW_LIMIT: f32 = 0.05; // in two places
    
    // Limit of Speed Limit
    if max_speed_float < MAX_SPEED_LOW_LIMIT {

        max_speed_float = MAX_SPEED_LOW_LIMIT;
        //println!("Max Speed below allowed limit: setting to {}%", max_speed_float * 100.0);
    }


    let speed_scale = config.get("Haptic_Config", "max_speed_scale").unwrap();
    let speed_scale_float: f32 = speed_scale.parse().unwrap();
    let speed_scale_float: f32 = speed_scale_float / 100.0;    


    let port_rx = config.get("Setup", "port_rx").unwrap();
    
    let proximity_parameter_address = config.get("Setup", "proximity_parameter").unwrap_or("/avatar/parameters/proximity_01".into());
    let max_speed_parameter_address = config.get("Setup", "max_speed_parameter").unwrap_or("/avatar/parameters/max_speed".into());

    // DeadZone Setup
    let deadzone_inner_string = config.get("Advanced_Haptic_Config", "dz_inner").unwrap();
    let deadzone_inner: f32 = deadzone_inner_string.parse().unwrap();
    println!("Deadzone_Inner: {}", deadzone_inner);

    let deadzone_outer_string = config.get("Advanced_Haptic_Config", "dz_outer").unwrap();
    let deadzone_outer: f32 = deadzone_outer_string.parse().unwrap();
    println!("Deadzone_Outer: {}", deadzone_outer);

    let dz_outer_address = config.get("dvanced_Haptic_Config", "dz_outer_address").unwrap_or("/avatar/parameters/deadzone_outer".into());
    let dz_inner_address = config.get("dvanced_Haptic_Config", "dz_inner_address").unwrap_or("/avatar/parameters/deadzone_inner".into());

    // Starting Banner

    println!("");
    banner_txt();
    println!("");
    println!(" Haptic Device: {}:{}", headpat_device_ip, headpat_device_port);
    println!(" Listening for OSC on port: {}", port_rx);
    println!("");
    println!(" Vibration Configuration");
    println!(" Min Speed: {}%", min_speed);
    println!(" Max Speed: {:?}%", max_speed_float*100.0);
    println!(" Scale Factor: {}%", speed_scale);
    println!("");    
    println!(" Waiting for pats...");
    
    // Return Tuple
    (
        headpat_device_ip,
        headpat_device_port,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        deadzone_inner,
        deadzone_outer,
        dz_outer_address,
        dz_inner_address,

    )

    
}




fn create_socket_address(host: &str, port: &str) -> String {
    
    // Define a function to create a socket address from a host and port
    // Create a new vector containing the host and port
    let address_parts = vec![host, port];
    // Join the parts together with a colon separator
    address_parts.join(":")
}

#[async_std::main]
async fn main() -> Result<()> {
     
    // Import Config 
    let (headpat_device_ip,
        headpat_device_port,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        mut deadzone_inner,
        mut deadzone_outer,
        dz_outer_address,
        dz_inner_address,
        

    ) = load_config();

    println!("Deadzone_Outer: {}", deadzone_outer);
    println!("Deadzone_Inner: {}", deadzone_inner);
    println!("Deadzone_Outer Address: {}", dz_outer_address);
    println!("Deadzone_Inner Address: {}", dz_inner_address);
    // // Setup Socket Address
    let rx_socket_address = create_socket_address("127.0.0.1", &port_rx);

    // Use the function to create the Tx socket address
    let tx_socket_address = create_socket_address(&headpat_device_ip, &headpat_device_port);
    
    // Connect to Tx socket
    let mut rx_socket = OscSocket::bind(rx_socket_address).await?;
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(tx_socket_address).await?; 

    // OSC Address Setup

    //const PROXIMITY_ADDRESS: &str = "/avatar/parameters/proximity_01";
    //const MAX_SPEED_ADDRESS: &str = "/avatar/parameters/max_speed";

    // New Device Addresses
    const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor";
    const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";


    // Deadzone Setup
    //let deadzone_outer = 0.25;
    //let deadzone_inner = 0.75;
 

    
    // Listen for incoming packets on the first socket.
    while let Some(packet) = rx_socket.next().await {

        let (packet, _peer_addr) = packet?;
        // Filter OSC Signals : Headpat Max & Headpat Prox 
        //let max_speed_address = create_address(&max_speed_parameter);

        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => {


                let (address, osc_value) = message.as_tuple();

                let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float(){
                    Some(v) => v, 
                    None => continue,
                };

                if address == max_speed_parameter_address {
                    
                    print_speed_limit(value); // print max speed limit
                    max_speed = value;
                    const MAX_SPEED_LOW_LIMIT: f32 = 0.05;  // this is in two places
                    if max_speed < MAX_SPEED_LOW_LIMIT {
                        max_speed = MAX_SPEED_LOW_LIMIT;
                    }
                }
                
                
                
                else if address == proximity_parameter_address  {

                    if value == 0.0 {
                        // Send 5 Stop Packets to Device
                        println!("Stopping pats...");
                    
                        for _ in 0..5 {
                            tx_socket
                                .send((TX_OSC_MOTOR_ADDRESS, (0i32,)))
                                .await?;
                        }
                    } else {
                        // Process Pat signal to send to Device
                        //println!("b4:{}", value);
                        let value = deadzone(value, deadzone_inner, deadzone_outer);
                        //println!("After:{}", value);
                        let motor_speed_tx = process_pat(value, max_speed, min_speed, speed_scale);
                        
                        tx_socket
                            .send((TX_OSC_MOTOR_ADDRESS, (motor_speed_tx,)))
                            .await?;
                    }

                }


                else if address == dz_outer_address  {
                    // Change DZ Outer Value
                    deadzone_outer = value;
                    println!("Outter Address {} Value {}", address, value);
                }

                else if address == dz_inner_address  {
                    // Change DZ Outer Value
                    deadzone_inner = value;
                    println!("Outter Address {} Value {}", address, value);
                }



                else {
                    //eprintln!("Unknown Address") // Have a debug mode, print if debug mode
                }

            }
            
        }  
   
    }
    Ok(())
}
