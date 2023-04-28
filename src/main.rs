// GiggleTech.io
// OSC Router
// by Sideways

// External crates

use async_osc::{prelude::*, OscPacket, OscType, Result};
use async_std::{
    stream::StreamExt,
    task::{self},
    sync::Arc,

};
use lazy_static::lazy_static;
use std::{sync::Mutex, time::{Duration, Instant}};
use std::sync::atomic::{AtomicBool, Ordering};

// Modules
mod data_processing;
mod config;
mod giggletech_osc;


// OSC Address Setup
const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor";
//const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";

// TimeOut 
lazy_static! {
    static ref LAST_SIGNAL_TIME: Mutex<Instant> = Mutex::new(Instant::now());
}

async fn osc_timeout(device_ip: &str) -> Result<()> {
    // If no new osc signal is Rx for 5s, will send stop packets
    // This loop can be used to implement Kays 'Soft Pat'
    loop {
        task::sleep(Duration::from_secs(1)).await;
        let elapsed_time = Instant::now().duration_since(*LAST_SIGNAL_TIME.lock().unwrap());

        if elapsed_time >= Duration::from_secs(5) {
            // Send stop packet
            println!("Pat Timeout...");
            giggletech_osc::send_data(device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;

            let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
            *last_signal_time = Instant::now();
        }
    }
}


/*
Signal Sender
To Start: start(running.clone(), running_mutex.clone()).await?;
to Stop : stop(running.clone(), running_mutex.clone()).await?;
 */


async fn start(running: Arc<AtomicBool>) -> Result<()> {
    if running.load(Ordering::SeqCst) {
        //return Err("Worker is already running".into());
    }
    running.store(true, Ordering::SeqCst);
    task::spawn(worker(running.clone(),"192.168.1.157" )); // ----------------------------------- FIX
    Ok(())
}


async fn worker(running: Arc<AtomicBool>, device_ip: &str) -> Result<()> {
    while running.load(Ordering::SeqCst) {
        println!("Worker is running");
        giggletech_osc::send_data(device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;
        task::sleep(Duration::from_secs(1)).await;
    }
    println!("Worker stopped");
    Ok(())
}

async fn stop(running: Arc<AtomicBool>) -> Result<()> {
    if !running.load(Ordering::SeqCst) {
        //return Err("Worker is not running".into());
    }
    running.store(false, Ordering::SeqCst);
    Ok(())
}



#[async_std::main]
async fn main() -> Result<()> {
     
    // Import Config 
    let (headpat_device_ip,
        _headpat_device_port,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        max_speed_low_limit,

    ) = config::load_config();

    // Rx/Tx Socket Setup
    let mut rx_socket = giggletech_osc::setup_rx_socket(port_rx).await?;

 
    // Timeout
    println!("IP: {}",headpat_device_ip);
    // I dont know why it needs to clone
    let headpat_device_ip_clone = headpat_device_ip.clone(); 
    task::spawn(async move {
        osc_timeout(&headpat_device_ip_clone).await.unwrap();
    });

    // Start/ Stop Function Setup
    let running = Arc::new(AtomicBool::new(false));
    let running_mutex = Arc::new(Mutex::new(()));

    // Listen for OSC Packets
    while let Some(packet) = rx_socket.next().await {
        let (packet, _peer_addr) = packet?;
        
        // Filter OSC Signals : Headpat Max & Headpat Prox 
        match packet {
            OscPacket::Bundle(_) => {}
            OscPacket::Message(message) => {

                let (address, osc_value) = message.as_tuple();
                let value = match osc_value.first().unwrap_or(&OscType::Nil).clone().float(){
                    Some(v) => v, 
                    None => continue,
                };

                // Max Speed Setting
                if address == max_speed_parameter_address {
                    data_processing::print_speed_limit(value);
                    max_speed = value.max(max_speed_low_limit);
                }
                
                // Prox Parmeter 
                else if address == proximity_parameter_address  {
                    
                    stop(running.clone()).await?;
                    // Update Last Signal Time for timeout clock
                    let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
                    *last_signal_time = Instant::now();

                    // Stop Function
                    if value == 0.0 {
                        // Send 5 Stop Packets to Device 
                        println!("Stopping pats...");
                        start(running.clone()).await?;

                        for _ in 0..5 {
                            giggletech_osc::send_data(&headpat_device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;  
                        }

                    } else {
                        giggletech_osc::send_data(&headpat_device_ip,
                            TX_OSC_MOTOR_ADDRESS,
                            data_processing::process_pat(value, max_speed, min_speed, speed_scale)).await?;
                    }
                }
                else {
                    //eprintln!("Unknown Address") // Have a debug mode, print if debug mode
                }
            } 
        }  
    }
    Ok(())
}
