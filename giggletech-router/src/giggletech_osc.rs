// giggletech_osc.rs

// GiggleTech OSC Module
// Data Sender, Tx & Rx Socket Setup

use async_osc::{ OscSocket, Result};

pub(crate) fn create_socket_address(host: &str, port: &str) -> String {
    let address_parts = vec![host, port];
    address_parts.join(":")
}

pub(crate) async fn setup_rx_socket(port: &str) -> Result<OscSocket> {
    let rx_socket_address = create_socket_address("127.0.0.1", &port.to_string());
    let rx_socket = OscSocket::bind(rx_socket_address).await?;
    Ok(rx_socket)
}

pub(crate) async fn setup_tx_socket(address: &str) -> Result<OscSocket> {
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(address).await?;
    Ok(tx_socket)
}


pub(crate) async fn send_data(device_ip: &str, motor_address: &str, value: i32) -> Result<()> {
    println!("Sending Value:{} to IP: {}", value, device_ip);
    
    // Todo 
    // Move socket connection out of send_data function
    // Notice no issues from setting up port upon every request at this point, have to move if there are any bug reports or memory leaks

    let tx_socket_address = create_socket_address(device_ip, "8888"); // ------------------- Port to Send OSC Data Too
    let tx_socket = setup_tx_socket(&tx_socket_address).await?;
    tx_socket.connect(tx_socket_address).await?;
    tx_socket.send((motor_address, (value,))).await?;

    Ok(())
}