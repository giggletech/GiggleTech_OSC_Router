/*
    Giggletech OSCQuery Server Initialization and Management Module

    This Rust module handles the initialization, management, and monitoring of the `giggletech_oscq.exe` process. The process is responsible 
    for running the Giggletech OSCQuery server. The module reads the necessary configuration from a YAML file, starts the OSCQuery server, 
    and retrieves the UDP port used for communication. If the process fails to start or the UDP port isn't valid, the process is restarted 
    automatically until a valid UDP port is retrieved.

    **Main Components:**
    1. **Config Reading:**
       - The configuration (e.g., the HTTP port) is read from a YAML file located in `AppData\Local\Giggletech\config_oscq.yml`.
    
    2. **Process Management:**
       - The Giggletech OSCQuery server process (`giggletech_oscq.exe`) is started by `run_giggletech()`, and its starting directory 
         is displayed in the console.
       - If the process fails to retrieve a valid UDP port or stops running, it is restarted automatically.

    3. **UDP Port Retrieval:**
       - The function `get_udp_port()` retrieves the UDP port from the OSCQuery server via an HTTP request. If the port is invalid (i.e., 0), 
         a start command is sent using `start_server()` to initialize the server properly.

    4. **Main Initialization Loop:**
       - The main function `initialize_and_get_udp_port()` continuously checks the UDP port, restarts the server process when necessary, 
         and returns the valid port once retrieved.

    **How It Works:**
    - First, the configuration is loaded from a YAML file.
    - Then, the OSCQuery process is started.
    - The module continuously tries to retrieve the UDP port from the server.
    - If the port is not valid (e.g., 0) or if any error occurs, the process is restarted, and the loop continues.
    - Once a valid UDP port is obtained, it is returned for use in the rest of the program.

    This module ensures that the OSCQuery server is always running and that the correct UDP port is available for communication.
*/





use std::fs;
use std::path::PathBuf;
use std::process::{Command, Child};
use std::thread::sleep;
use std::time::Duration;
use dirs::data_local_dir;
use serde::Deserialize;
use reqwest::blocking::Client;
use serde_yaml;

// Struct to deserialize the YAML config
#[derive(Debug, Deserialize)]
struct Config {
    httpPort: u16,
}

// Function to read and parse the YAML config file
fn read_config() -> Config {
    let mut config_path = data_local_dir().expect("Failed to get AppData\\Local directory");
    config_path.push("Giggletech");
    config_path.push("config_oscq.yml");

    let config_str = fs::read_to_string(config_path).expect("Failed to read config_oscq.yml");
    serde_yaml::from_str(&config_str).expect("Failed to parse config_oscq.yml")
}

// Function to start the giggletech process
fn run_giggletech() -> Child {
    let mut executable_path = data_local_dir().expect("Failed to get AppData\\Local directory");
    executable_path.push("Giggletech");
    executable_path.push("giggletech_oscq.exe");

    // Display a message indicating the process is being started and the directory it's being started from
    println!(
        "Starting OSCQ Server from the directory: {}",
        executable_path.display()
    );

    Command::new(executable_path)
        .spawn()
        .expect("Failed to start giggletech process")
}


// Function to get the UDP port from the server (synchronous)
fn get_udp_port(port: u16) -> Result<i32, reqwest::Error> {
    let url = format!("http://localhost:{}/port_udp", port);
    let response = reqwest::blocking::get(&url)?;
    let port_value: i32 = response.text()?.trim().parse().unwrap_or(0); // Return 0 if parsing fails
    Ok(port_value)
}

// Function to send the start command to the server (synchronous)
fn start_server(port: u16) -> Result<(), reqwest::Error> {
    let client = Client::new();
    let url = format!("http://localhost:{}/start", port);
    client.get(&url).send()?.error_for_status()?;
    Ok(())
}

// Function to initialize, handle the giggletech process, and return the UDP port (synchronous)
pub fn initialize_and_get_udp_port() -> i32 {
    // Step 1: Read the configuration
    let config = read_config();

    // Step 2: Start the giggletech process
    let mut process = run_giggletech();

    // Step 3: Loop until we get a non-zero UDP port
    loop {
        match get_udp_port(config.httpPort) {
            Ok(0) => {
                // If UDP port is 0, send the start command
                println!("UDP port is 0, sending start command...");
                if let Err(e) = start_server(config.httpPort) {
                    eprintln!("Failed to start server: {}", e);
                }
            }
            Ok(port_value) => {
                // If we get a valid non-zero port, return it
                println!("UDP port: {}", port_value);
                return port_value;
            }
            Err(_) => {
                // If the request fails, restart the process
                eprintln!("Failed to retrieve UDP port, restarting giggletech process...");
                let _ = process.kill(); // Kill the current process
                process = run_giggletech(); // Restart the process
            }
        }

        // Sleep before the next check
        sleep(Duration::from_secs(1));
    }
}
