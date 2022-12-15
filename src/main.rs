// Headpat IO 
// by Sideways / Jason Beattie
// OSC Setup
// working but roll back anyway

use async_osc::{prelude::*, OscPacket, OscSocket, OscType, Result};
use async_std::stream::StreamExt;
use configparser::ini::Ini;
use std::collections::HashMap;

fn proximity_graph(proximity_signal: f32){
    // Not quite working, need to loop it
    let mut pat_meter = String::new();

    println!("prox {}", proximity_signal);
    if proximity_signal < 0.1{
        pat_meter = String::from("|          |");
    }

    println!("prox {}", proximity_signal);
    if proximity_signal < 0.2{
        pat_meter = String::from("|          |");
    }
    let proximity_signal =  proximity_signal * 10.0;
    let proximity_signal = proximity_signal.round() as i32;

    println!("{}", proximity_signal);

}

fn process_pat(proximity_signal: f32, max_speed: f32, min_speed: f32, speed_scale: f32) -> i32 {
    //proximity_graph(proximity_signal);
    // Process the proximetery signal to a motor speed signal
    const MOTOR_SPEED_SCALE: f32 = 0.66; // Motor is being powered off the 5v rail, rated for 3.3v

    let headpat_delta:f32 = max_speed - min_speed; // Take the differance, so when at low proximetery values, the lowest value still buzzes the motor                      
    let headpat_tx = headpat_delta * proximity_signal + min_speed;
    
    let headpat_tx = headpat_tx * MOTOR_SPEED_SCALE * speed_scale* 255.0;
    let headpat_tx = headpat_tx as i32;
    let proximity_signal = format!("{:.2}", proximity_signal);
    let max_speed = format!("{:.2}", max_speed);

    
    if headpat_tx > 99{
        eprintln!("Prox: {} Motor Tx: {} Max Speed:{}", proximity_signal, headpat_tx, max_speed);
    }
    else{
        eprintln!("Prox: {} Motor Tx: {} Max Speed:{}:", proximity_signal, headpat_tx, max_speed);
    }
    
    
    headpat_tx
}

fn banner_txt(){

    println!("888    888                        888                   888        8888888  .d88888b.  ");
    println!("888    888                        888                   888          888   d88P   Y88b "); 
    println!("888    888                        888                   888          888   888     888 ");
    println!("8888888888  .d88b.   8888b.   .d88888 88888b.   8888b.  888888       888   888     888 "); 
    println!("888    888 d8P  Y8b      88b d88  888 888  88b      88b 888          888   888     888 ");
    println!("888    888 88888888 .d888888 888  888 888  888 .d888888 888          888   888     888 ");
    println!("888    888 Y8b.     888  888 Y88b 888 888 d88P 888  888 Y88b.        888   Y88b. .d88P ");
    println!("888    888   Y8888   Y888888   Y88888 88888P    Y888888   Y888     8888888   Y88888P   ");
    println!("                                      888                                              ");
    println!("                                      888                                              ");
    println!("by Sideways                           888                                              ");

}

fn load_config() -> (String, String, f32, f32, f32, String, String, String, String, String) {
    let mut config = Ini::new();

    match config.load("./config.ini") {
        Err(why) => panic!("{}", why),
        Ok(_) => {}
    }

    let map = match config.get_map() {
        None => HashMap::new(),
        Some(map) => map,
    };

    let headpat_device_ip = config.get("Device_Setup", "headpat_io_ip").unwrap();
    let headpat_device_port = config.get("Device_Setup", "headpat_io_port").unwrap();

    let min_speed = config.get("Haptic_Setup", "min_speed").unwrap();
    let min_speed_float: f32 = min_speed.parse().unwrap();
    let min_speed_float: f32 = min_speed_float / 100.0;
    

    let max_speed = config.get("Haptic_Setup", "max_speed").unwrap();
    let max_speed_float: f32 = max_speed.parse().unwrap();
    let max_speed_float: f32 = max_speed_float / 100.0;

    let speed_scale = config.get("Haptic_Setup", "max_speed_scale").unwrap();
    let speed_scale_float: f32 = speed_scale.parse().unwrap();
    let speed_scale_float: f32 = speed_scale_float / 100.0;    


    let port_rx = config.get("OSC_Setup", "port_rx").unwrap();
    let proximity_parameter = config.get("OSC_Setup", "proximity_parameter").unwrap();
    let max_speed_parameter = config.get("OSC_Setup", "max_speed_parameter").unwrap();

    let ch_1_address = config.get("OSC_Setup", "ch_1_address").unwrap();
    let ch_2_address = config.get("OSC_Setup", "ch_2_address").unwrap();


    
    println!("");
    banner_txt(); // Print Banner
    println!("");
    println!("Headpat Device: {}:{}", headpat_device_ip, headpat_device_port);
    println!("");
    println!("Vibration Configuration");
    println!("Min Speed: {}%", min_speed);
    println!("Max Speed: {}%", max_speed);
    println!("Speed Scaling: {}%", speed_scale);
    println!("");    
    println!("OSC Configuration");
    println!("Listening for OSC on port: {}", port_rx);
    println!("Headpat proximity parameter name: {}", proximity_parameter); 
    println!("Max Speed parameter name: {}", max_speed_parameter);
    println!(""); 
    //println!("Headpat Motor OSC address: {}", ch_1_address);
    //println!("Headpat LED OSC address: {}", ch_2_address);
    //println!("");
    println!("Waiting for pats...");
    
    // Return Tuple
    (
        headpat_device_ip,
        headpat_device_port,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameter,
        max_speed_parameter,
        ch_1_address,
        ch_2_address,
    )
}

fn create_address(parameter: &str) -> String {

    let avatar_address = "/avatar/parameters/";
    // Create a new vector containing the avatar address and the parameter
    let address_parts = vec![avatar_address.to_string(), parameter.to_string()];
    // Join the parts together with no separator
    let address = address_parts.join("");
    // Trim the address to remove any leading or trailing white space
    address.trim().to_string()
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
        proximity_parameter,
        max_speed_parameter,
        ch_1_address,
        ch_2_address
    ) = load_config();


    // // Setup Socket Address
    let rx_socket_address = create_socket_address("127.0.0.1", &port_rx);

    // Use the function to create the Tx socket address
    let tx_socket_address = create_socket_address(&headpat_device_ip, &headpat_device_port);
    
    // Connect to Tx socket
    let mut rx_socket = OscSocket::bind(rx_socket_address).await?;
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(tx_socket_address).await?; 


    // Headpat Constants
    

    // OSC Address Setup

    // Setup from config file not working

    // let proximity_address = create_address( &proximity_parameter); 
    // println!("prod add{}", proximity_address);
    // let max_speed_address = create_address(&max_speed_parameter);
    // println!("maxa speed{}", max_speed_address);
    // Setup Tx OSC Address  NOT WORKING                                  
    //let tx_osc_address_1 = ch_1_address.to_string();
    //let tx_osc_address_2 = ch_2_address.to_string();
    
    // Address Setup Not working, exclude for now cuze its working - but I need to be able to change these
    // these carnt be constants becuase the config will need to load new var
    const MAX_SPEED_ADDRESS: &str = "/avatar/parameters/Headpat_max";
    const PROXIMITY_ADDRESS: &str = "/avatar/parameters/Headpat_prox_1";

    // Old Device Addresses
    //const TX_OSC_ADDRESS_1: &str = "/avatar/parameters/Headpat_prox_0";
    //const TX_OSC_ADDRESS_2: &str = "/avatar/parameters/Headpat_prox_1";

    // New Device Addresses
    const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor";
    const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";
    

    // Listen for incoming packets on the first socket.
    while let Some(packet) = rx_socket.next().await {

        let (packet, peer_addr) = packet?;
        // Filter OSC Signals : Headpat Max & Headpat Prox 
        //let max_speed_address = create_address(&max_speed_parameter);

        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => match &message.as_tuple() {
                (MAX_SPEED_ADDRESS, &[OscType::Float(max_speed_rx)]) => {
                    
                    max_speed = max_speed_rx;
                    let max_speed = format!("{:.2}", max_speed);
                    eprintln!("Headpat Max Speed: {}", max_speed);
                }
                (PROXIMITY_ADDRESS, &[OscType::Float(proximity_reading)]) => {
                    if proximity_reading == 0.0 {
                        // Send 5 Stop Packets to Device
                        println!("Stopping pats...");
        
                        for _ in 0..5 {
                            tx_socket
                                .send((TX_OSC_MOTOR_ADDRESS, (0i32,)))
                                .await?;
                        }
                    } else {
                        // Process Pat signal to send to Device   
                        let motor_speed_tx = process_pat(proximity_reading, max_speed, min_speed, speed_scale);
                        
        
                        tx_socket
                            .send((TX_OSC_MOTOR_ADDRESS, (motor_speed_tx,)))
                            .await?;
                    }
                }
                _ => {}
            },
        }  
        
        



















        
    }
    Ok(())
}
