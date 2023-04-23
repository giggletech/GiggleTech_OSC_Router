// GiggleTech.io
// OSC Router
// by Sideways

// External crates

use regex::Regex;
use async_osc::{prelude::*, OscPacket, OscSocket, OscType, Result};
use async_std::{
    stream::StreamExt,
    task::{self},
    sync::Arc,

};
use configparser::ini::Ini;
use lazy_static::lazy_static;
use std::{sync::Mutex, time::{Duration, Instant}};
use std::sync::atomic::{AtomicBool, Ordering};
use futures::future;


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

fn load_config() -> (
    Vec<String>, // headpat_device_ip
    String, // headpat_device_default_port
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

    let device_uri_sep = Regex::new(r"\s+").expect("Invalid regex");


    //let headpat_device_uris: Vec<String> = config.get("Setup", "device_uris").unwrap()
    //    .split_whitespace().collect();


    let headpat_device_uris: Vec<String> = config.get("Setup", "device_uris").unwrap()
    .split_whitespace()
    .map(|s| s.to_string()) // convert &str to String
    .collect();



    let headpat_device_default_port = "8888".to_string();
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
    println!(" Haptic Device: {:?}:{:?}", headpat_device_uris, headpat_device_default_port);
    println!(" Listening for OSC on port: {}", port_rx);
    println!("\n Vibration Configuration");
    println!(" Min Speed: {}%", min_speed);
    println!(" Max Speed: {:?}%", max_speed_float * 100.0);
    println!(" Scale Factor: {}%", speed_scale);
    println!("\nWaiting for pats...");

    (
        headpat_device_uris,
        headpat_device_default_port,
        min_speed_float,
        max_speed_float,
        speed_scale_float,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        max_speed_low_limit,
    )
}

// Make it easy to see prox when looking at router
fn proximity_graph(proximity_signal: f32) -> String {
    let num_dashes = (proximity_signal * 10.0) as usize;
    let graph = "-".repeat(num_dashes) + ">";

    graph
}

fn print_speed_limit(headpat_max_rx: f32) {
    let headpat_max_rx_print = (headpat_max_rx * 100.0).round() as i32;
    let max_meter = match headpat_max_rx_print {
        91..=i32::MAX => "!!! SO MUCH !!!",
        76..=90 => "!! ",
        51..=75 => "!  ",
        _ => "   ",
    };
    println!("Speed Limit: {}% {}", headpat_max_rx_print, max_meter);
}

// Pat Processor
const MOTOR_SPEED_SCALE: f32 = 0.66; // Overvolt   Here, OEM config 0.66 going higher than this value will reduce your vibrator motor life
fn process_pat(proximity_signal: f32, max_speed: f32, min_speed: f32, speed_scale: f32) -> i32 {
    let graph_str = proximity_graph(proximity_signal);
    let headpat_tx = (((max_speed - min_speed) * proximity_signal + min_speed) * MOTOR_SPEED_SCALE * speed_scale * 255.0).round() as i32;
    let proximity_signal = format!("{:.2}", proximity_signal);
    let max_speed = format!("{:.2}", max_speed);
    eprintln!("Prox: {:5} Motor Tx: {:3}  Max Speed: {:5} |{:11}|", proximity_signal, headpat_tx, max_speed, graph_str);

    headpat_tx
}


// Tx & Rx Socket Setup

fn create_socket_address(host: &str, port: &str) -> String {
    let address_parts = vec![host, port];
    address_parts.join(":")
}

async fn setup_rx_socket(port: std::string::String) -> Result<OscSocket> {
    let rx_socket_address = create_socket_address("127.0.0.1", &port.to_string());
    let rx_socket = OscSocket::bind(rx_socket_address).await?;
    Ok(rx_socket)
}

async fn setup_tx_socket(address: std::string::String) -> Result<OscSocket> {
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(address).await?;
    Ok(tx_socket)
}





// OSC Address Setup
const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor";
//const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";



// TimeOut
lazy_static! {
    static ref LAST_SIGNAL_TIME: Mutex<Instant> = Mutex::new(Instant::now());
}

async fn osc_timeout(tx_socket: OscSocket) -> Result<()> {
    // If no new osc signal is Rx for 5s, will send stop packets
    loop {
        task::sleep(Duration::from_secs(1)).await;
        let elapsed_time = Instant::now().duration_since(*LAST_SIGNAL_TIME.lock().unwrap());

        if elapsed_time >= Duration::from_secs(5) {
            // Send stop packet
            println!("Pat Timeout...");
            tx_socket.send((TX_OSC_MOTOR_ADDRESS, (0i32,))).await?;

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


async fn start(
    running: Arc<AtomicBool>,
    running_mutex: Arc<Mutex<()>>,
) -> Result<()> {
    let _lock = running_mutex.lock();
    if running.load(Ordering::SeqCst) {
        //return Err(async_osc::Error::from("Worker is already running".to_string()));

    }
    running.store(true, Ordering::SeqCst);
    task::spawn(worker(running.clone()));
    Ok(())
}

async fn worker(running: Arc<AtomicBool>) -> Result<()> {
    while running.load(Ordering::SeqCst) {
        // Do some work here
        println!("Worker is running");
        // ADD STOP COMMANDS HERE
        //task::sleep(Duration::from_secs(1)).await;
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


#[async_std::main]
async fn main() -> Result<()> {

    // Import Config
    let (headpat_device_uris,
        headpat_device_default_port,
        min_speed,
        mut max_speed,
        speed_scale,
        port_rx,
        proximity_parameter_address,
        max_speed_parameter_address,
        max_speed_low_limit,

    ) = load_config();

    // Rx/Tx Socket Setup
    let mut rx_socket = setup_rx_socket(port_rx).await?;

    // Bind to remote hardware UDP sockets
    // can send values to a specifed IP, Address, and value, which is called when needed
    


/* 
    let tx_sockets: Vec<_> = headpat_device_uris.iter()
      .map(|device_uri| {
        //let device_parts = Regex::new(r":").unwrap().split(device_uri);
        let device_parts: Vec<_> = Regex::new(r":").unwrap().split(device_uri).collect();

        //let tx_socket_address = create_socket_address(device_parts[0], &device_parts.get(1).unwrap_or_else(|| headpat_device_default_port));
        let tx_socket_address = create_socket_address(
            device_parts.get(0).unwrap(),
            &device_parts.get(1).map(|s| *s).unwrap_or_else(|| &headpat_device_default_port)
        );
        
        

        println!("Connecting to harware device: {:?}", tx_socket_address);

        let tx_socket = setup_tx_socket(tx_socket_address);

        // schedule background thread for timeout to send disconnection notification packets to remote device
        // Remove to get it working
        //task::spawn(osc_timeout(tx_socket));


        tx_socket
      })



      .collect();
*/

// This fixed the problems but spawm more :S Now same as code block above

    let tx_sockets = headpat_device_uris.iter()
        .map(|device_uri| async move {
            let device_parts: Vec<_> = Regex::new(r":").unwrap().split(device_uri).collect();

            let tx_socket_address = create_socket_address(
                device_parts.get(0).unwrap(),
                &device_parts.get(1).map(|s| *s).unwrap_or_else(|| &headpat_device_default_port)
            );

            println!("Connecting to harware device: {:?}", tx_socket_address);

            let tx_socket = setup_tx_socket(tx_socket_address).await?;
            task::spawn(osc_timeout(tx_socket)).await;

            Ok(tx_socket)
        })
        .collect::<Vec<_>>();

    let mut joined_sockets = futures::future::join_all(tx_sockets).await;






    future::join_all(tx_sockets).await;
    println!("All hardware connections established.");

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
                    print_speed_limit(value);
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
//                             proximity_parameter_output_address
                            tx_sockets.iter().for_each(|socket| {
                                socket.send((TX_OSC_MOTOR_ADDRESS, (0i32,)));
                            })
                        }

                    } else {
                        // Process Pat signal to send to Device
                        let motor_speed_tx = process_pat(value, max_speed, min_speed, speed_scale);

                        tx_sockets.iter().for_each(|socket| {
                            socket.send((TX_OSC_MOTOR_ADDRESS, (motor_speed_tx,)));
                        })
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
