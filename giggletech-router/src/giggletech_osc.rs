// giggletech_osc.rs

// GiggleTech OSC Module
// Data Sender, Tx & Rx Socket Setup

use async_osc::{ OscSocket, Result};

// OSC Address Setup
const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor"; 
const TX_OSC_GIGGLESPARK: &str = "/motor"; 

const TX_OSC_COLLAR_1: &str = "/motor";
const TX_OSC_COLLAR_2: &str = "/motor_02";
const TX_OSC_COLLAR_3: &str = "/motor_03";
const TX_OSC_COLLAR_4: &str = "/motor_04";
//const TX_OSC_LED_ADDRESS_2: &str = "/avatar/parameters/led";

pub(crate) fn create_socket_address(host: &str, port: &str) -> String {
    let address_parts = vec![host, port];
    address_parts.join(":")
}

pub(crate) async fn setup_rx_socket(port: std::string::String) -> Result<OscSocket> {
    let rx_socket_address = create_socket_address("127.0.0.1", &port.to_string());
    let rx_socket = OscSocket::bind(rx_socket_address).await?;
    Ok(rx_socket)
}

pub(crate) async fn setup_tx_socket(address: std::string::String) -> Result<OscSocket> {
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(address).await?;
    Ok(tx_socket)
}


pub(crate) async fn send_data(device_ip: &str, value: i32) -> Result<()> {
    //println!("Sending Value:{} to IP: {}", value, device_ip);
    
    // Todo 
    // Move socket connection out of send_data function
    // Notice no issues from setting up port upon every request at this point, have to move if there are any bug reports or memory leaks

    let tx_socket_address = create_socket_address(device_ip, "8888"); // ------------------- Port to Send OSC Data Too
    let tx_socket = setup_tx_socket(tx_socket_address.clone()).await?;
    tx_socket.connect(tx_socket_address).await?;
    tx_socket.send((TX_OSC_MOTOR_ADDRESS, (value,))).await?;
    //tx_socket.send((TX_OSC_GIGGLESPARK, (value,))).await?;

    tx_socket.send((TX_OSC_COLLAR_1, (value,))).await?; // Can TX to the diff channels
    tx_socket.send((TX_OSC_COLLAR_2, (value,))).await?; // Can TX to the diff channels

    tx_socket.send((TX_OSC_COLLAR_3, (value,))).await?; // Can TX to the diff channels

    tx_socket.send((TX_OSC_COLLAR_4, (value,))).await?; // Can TX to the diff channels



    Ok(())
}