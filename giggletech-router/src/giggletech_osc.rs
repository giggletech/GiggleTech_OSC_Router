// giggletech_osc.rs

// GiggleTech OSC Module
// Tx  Socket Setup

use async_osc::{ OscSocket, Result};


pub(crate) fn create_socket_address(host: &str, port: &str) -> String {
    let address_parts = vec![host, port];
    address_parts.join(":")
}

pub(crate) async fn setup_tx_socket(address: std::string::String) -> Result<OscSocket> {
    let tx_socket = OscSocket::bind("0.0.0.0:0").await?;
    tx_socket.connect(address).await?;
    Ok(tx_socket)
}


pub(crate) async fn send_data(device_ip: &str, osc_address: &str, port: &str, value: f32) -> Result<()> {
    //println!("Sending Value:{} to IP: {}", value, device_ip);
    let tx_socket_address = create_socket_address(device_ip, port); // ------------------- Port to Send OSC Data Too
    let tx_socket = setup_tx_socket(tx_socket_address.clone()).await?;
    tx_socket.connect(tx_socket_address).await?;
    tx_socket.send((osc_address, (value,))).await?;
    Ok(())
}