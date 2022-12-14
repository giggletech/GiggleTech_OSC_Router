// Headpat IO 
// by Sideways / Jason Beattie

// Need to get the importing of IP stuff working

// OSC Setup

use async_osc::{prelude::*, OscPacket, OscSocket, OscType, Result};
use async_std::stream::StreamExt;

// config.ini setup

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

fn process_pat(proximity_signal: f32, max_speed: f32, min_speed: f32) -> i32 {
    //proximity_graph(proximity_signal);
    // Process the proximetery signal to a motor speed signal
    let headpat_delta:f32 = max_speed - min_speed; // Take the differance, so when at low proximetery values, the lowest value still buzzes the motor                      
    let headpat_tx = headpat_delta * proximity_signal + min_speed;
    let headpat_tx = headpat_tx * 255.0;
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

fn load_config() -> (String, f32, f32, String, String, String) {
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

    let min_speed = config.get("Haptic_Setup", "min_speed").unwrap();
    let min_speed_float: f32 = min_speed.parse().unwrap();
    let min_speed_float: f32 = min_speed_float / 100.0;
    

    let max_speed = config.get("Haptic_Setup", "max_speed").unwrap();
    let max_speed_float: f32 = max_speed.parse().unwrap();
    let max_speed_float: f32 = max_speed_float / 100.0;


    let port_rx = config.get("OSC_Setup", "port_rx").unwrap();
    let proximity_parameter = config.get("OSC_Setup", "proximity_parameter").unwrap();
    let max_speed_parameter = config.get("OSC_Setup", "max_speed_parameter").unwrap();

    // Print Banner
    banner_txt();
    println!("");
    println!("Headpat Device IP: {}", headpat_device_ip);
    println!("");
    println!("Vibration Configuration");
    println!("Min Speed: {}%", min_speed);
    println!("Max Speed: {}%", max_speed);
    println!("");    
    println!("OSC Configuration");
    println!("Listening for OSC on port: {}", port_rx);
    println!("Headpat proximity parameter name: {}", proximity_parameter);
    println!("Max Speed parameter name: {}", max_speed_parameter);
    println!("");
    println!("Waiting for pats...");
    
    // Return Tuple
    (
        headpat_device_ip,
        min_speed_float,
        max_speed_float,
        port_rx,
        proximity_parameter,
        max_speed_parameter,
    )
}

#[async_std::main]
async fn main() -> Result<()> {
     
    // Import Config 
    let (headpat_device_ip, min_speed, mut max_speed, port_rx, proximity_parameter, max_speed_parameter) = load_config();

    // Setup Rx Socket                          
    let rx_socket_address  = vec!["127.0.0.1", &port_rx];
    let rx_socket_address = rx_socket_address.join(":");
    let mut rx_socket = OscSocket::bind(rx_socket_address).await?;

    // Setup Tx Socket
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    let tx_socket_address = vec![headpat_device_ip.to_string(), "8888".to_string()]; //----------------------------------------- Headpat Device Port Setup / default 8888
    let tx_socket_address = tx_socket_address.join(":");

    tx_socket.connect(tx_socket_address).await?;

    // Listen for incoming packets on the first socket.
    while let Some(packet) = rx_socket.next().await {

        let (packet, _peer_addr) = packet?;
        // Filter OSC Signals : Headpat Max & Headpat Prox 
        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => match &message.as_tuple() {
                ("/avatar/parameters/Headpat_max", &[OscType::Float(max_speed_rx)]) => {
                    
                    max_speed = max_speed_rx;
                    let max_speed = format!("{:.2}", max_speed);
                    eprintln!("Headpat Max Speed: {}", max_speed);
                }
                ("/avatar/parameters/Headpat_prox_1", &[OscType::Float(proximity_reading)]) => {
                    if proximity_reading == 0.0 {
                        // Send 5 Stop Packets to Device
                        println!("Stopping pats...");
        
                        for _ in 0..5 {
                            tx_socket
                                .send(("/avatar/parameters/Headpat_prox_1", (0i32,)))
                                .await?;
                        }
                    } else {
                        // Process Pat signal to send to Device   
                        let motor_speed_tx = process_pat(proximity_reading, max_speed, min_speed);
        
                        tx_socket
                            .send(("/avatar/parameters/Headpat_prox_1", (motor_speed_tx,)))
                            .await?;
                    }
                }
                _ => {}
            },
        }         
    }
    Ok(())
}
