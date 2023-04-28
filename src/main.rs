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
mod socket_setup;



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
            send_data(device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;

            let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
            *last_signal_time = Instant::now();
        }
    }
}


/*
0 Signal Sender

To Start: start(running.clone(), running_mutex.clone()).await?;
to Stop : stop(running.clone(), running_mutex.clone()).await?;
 */


async fn start(running: Arc<AtomicBool>, running_mutex: Arc<Mutex<()>>) -> Result<()> {

    let _lock = running_mutex.lock();
    if running.load(Ordering::SeqCst) {
        //return Err(async_osc::Error::from("Worker is already running".to_string()));
    }
    running.store(true, Ordering::SeqCst);
    task::spawn(worker(running.clone(),"192.168.1.157" )); // ----------------------------------------------- FIX
    
    Ok(())
}


async fn worker(running: Arc<AtomicBool>, device_ip: &str) -> Result<()> {
    while running.load(Ordering::SeqCst) {
        // Do some work here
        println!("Worker is running");

        // Send stop command
        send_data(device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;

        task::sleep(Duration::from_secs(1)).await;
    }
    println!("Worker stopped");
    Ok(())
}



async fn stop(
    running: Arc<AtomicBool>,
    running_mutex: Arc<Mutex<()>>,
) -> Result<()> {
    let _lock = running_mutex.lock();
    if !running.load(Ordering::SeqCst) {
        //return Err("Worker is not running".into());
    }
    running.store(false, Ordering::SeqCst);
    Ok(())
}





// OSC Address Setup
const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor";
//const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";


async fn send_data(device_ip: &str, address: &str, value: i32) -> Result<()> {
    println!("Sending Value:{} to IP: {}", value, device_ip);
    let tx_socket_address = socket_setup::create_socket_address(device_ip, "8888"); // ------------------- Port to Send OSC Data Too
    let tx_socket = socket_setup::setup_tx_socket(tx_socket_address.clone()).await?;
    tx_socket.connect(tx_socket_address).await?;
    tx_socket.send((address, (value,))).await?;
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
    let mut rx_socket = socket_setup::setup_rx_socket(port_rx).await?;

 
    // Timeout
    println!("IP: {}",headpat_device_ip);
    // I dont know why it needs to clone
    let headpat_device_ip_clone = headpat_device_ip.clone(); 
    task::spawn(async move {
        osc_timeout(&headpat_device_ip_clone).await;
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
                    
                    stop(running.clone(), running_mutex.clone()).await?;
                    // Update Last Signal Time for timeout clock
                    let mut last_signal_time = LAST_SIGNAL_TIME.lock().unwrap();
                    *last_signal_time = Instant::now();

                    // Stop Function
                    if value == 0.0 {
                        // Send 5 Stop Packets to Device - need to update so it sends stop packets until a new prox signal is made

                        println!("Stopping pats...");
                        start(running.clone(), running_mutex.clone()).await?;

                        for _ in 0..5 {
                            send_data(&headpat_device_ip, TX_OSC_MOTOR_ADDRESS, 0i32).await?;
                            
                        }

                    } else {

                        //let motor_speed_tx = data_processing::process_pat(value, max_speed, min_speed, speed_scale);
                        send_data(&headpat_device_ip,
                            TX_OSC_MOTOR_ADDRESS,
                            data_processing::process_pat(value, max_speed, min_speed, speed_scale)).await?;
                        

                    }
                }
                else {
                    eprintln!("Unknown Address") // Have a debug mode, print if debug mode
                }
            } 
        }  
    }
    Ok(())
}
