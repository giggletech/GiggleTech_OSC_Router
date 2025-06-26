// giggletech_osc.rs
/*
    giggletech_osc.rs - GiggleTech OSC Module

    This module handles sending and receiving OSC (Open Sound Control) messages using sockets.
    It implements an elegant connection manager for efficient socket handling with proper
    timeouts, error handling, and resource management.

    **Key Features:**
    
    1. **Connection Manager**: Tracks connection statistics and manages socket lifecycle
    2. **Timeout Handling**: Proper timeouts for connection and send operations
    3. **Error Recovery**: Graceful handling of network errors
    4. **Resource Management**: Automatic cleanup of stale connections
    5. **Statistics**: Connection monitoring and debugging capabilities

    **Usage:**
    - Use `setup_rx_socket` for receiving OSC messages
    - Use `send_data` for sending OSC messages with automatic connection management
    - Call `start_connection_manager()` to enable automatic cleanup
*/

use async_osc::{ OscSocket, Result};
use std::collections::HashMap;
use std::sync::Arc;
use async_std::sync::RwLock;
use std::time::{Duration, Instant};

// Connection manager for efficient socket handling
pub struct ConnectionManager {
    connections: Arc<RwLock<HashMap<String, ConnectionInfo>>>,
}

#[derive(Clone)]
struct ConnectionInfo {
    last_used: Instant,
    connection_count: u32,
    success_count: u32,
    error_count: u32,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // Update connection info after communication attempt
    async fn update_connection_info(&self, device_ip: &str, success: bool) {
        let mut connections = self.connections.write().await;
        let info = connections.entry(device_ip.to_string()).or_insert(ConnectionInfo {
            last_used: Instant::now(),
            connection_count: 0,
            success_count: 0,
            error_count: 0,
        });
        
        info.last_used = Instant::now();
        info.connection_count += 1;
        
        if success {
            info.success_count += 1;
        } else {
            info.error_count += 1;
        }
    }

    // Cleanup old connections
    async fn cleanup_old_connections(&self) {
        let timeout = Duration::from_secs(300); // 5 minutes
        let now = Instant::now();
        let mut connections = self.connections.write().await;
        
        let before_count = connections.len();
        connections.retain(|_, info| {
            now.duration_since(info.last_used) < timeout
        });
        let after_count = connections.len();
        
        if before_count != after_count {
            println!("Cleaned up {} stale connections", before_count - after_count);
        }
    }

    // Get connection statistics
    pub async fn get_stats(&self) -> HashMap<String, (u32, u32, u32)> {
        let connections = self.connections.read().await;
        connections.iter()
            .map(|(ip, info)| (ip.clone(), (info.connection_count, info.success_count, info.error_count)))
            .collect()
    }
}

// Global connection manager instance
lazy_static::lazy_static! {
    static ref CONNECTION_MANAGER: ConnectionManager = ConnectionManager::new();
}

// OSC Address Setup
const TX_OSC_MOTOR_ADDRESS: &str = "/avatar/parameters/motor"; // legacy support
const TX_OSC_GIGGLESPARK: &str = "/motor"; // both gigglepuck and spark use this
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

// Start connection manager cleanup task
pub(crate) async fn start_connection_manager() {
    println!("Starting connection manager with automatic cleanup...");
    async_std::task::spawn(async {
        loop {
            async_std::task::sleep(Duration::from_secs(60)).await; // Cleanup every minute
            CONNECTION_MANAGER.cleanup_old_connections().await;
        }
    });
}

// Send data with proper connection management and timeouts
pub(crate) async fn send_data(device_ip: &str, value: i32) -> Result<()> {
    let socket_address = create_socket_address(device_ip, "8888");
    
    // Create socket with connection timeout
    let socket = match async_std::future::timeout(
        Duration::from_secs(2), // 2 second connection timeout
        setup_tx_socket(socket_address.clone())
    ).await {
        Ok(Ok(socket)) => socket,
        Ok(Err(e)) => {
            CONNECTION_MANAGER.update_connection_info(device_ip, false).await;
            return Err(e);
        }
        Err(_) => {
            CONNECTION_MANAGER.update_connection_info(device_ip, false).await;
            return Err(async_osc::Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Connection timeout to {}", device_ip)
            )));
        }
    };

    // Send OSC messages with send timeout
    let send_result = async_std::future::timeout(
        Duration::from_secs(1), // 1 second send timeout
        async {
            socket.send((TX_OSC_MOTOR_ADDRESS, (value,))).await?;
            socket.send((TX_OSC_GIGGLESPARK, (value,))).await?;
            Ok::<(), async_osc::Error>(())
        }
    ).await;

    match send_result {
        Ok(Ok(())) => {
            // Success - update connection info
            CONNECTION_MANAGER.update_connection_info(device_ip, true).await;
            Ok(())
        }
        Ok(Err(e)) => {
            // Send error
            CONNECTION_MANAGER.update_connection_info(device_ip, false).await;
            Err(e)
        }
        Err(_) => {
            // Send timeout
            CONNECTION_MANAGER.update_connection_info(device_ip, false).await;
            Err(async_osc::Error::Io(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                format!("Send timeout to {}", device_ip)
            )))
        }
    }
}

// Get connection statistics for monitoring
pub(crate) async fn get_connection_stats() -> HashMap<String, (u32, u32, u32)> {
    CONNECTION_MANAGER.get_stats().await
}

// Print connection statistics
pub(crate) async fn print_connection_stats() {
    let stats = get_connection_stats().await;
    if !stats.is_empty() {
        println!("\n=== Connection Statistics ===");
        for (device_ip, (total, success, errors)) in stats {
            let success_rate = if total > 0 { (success as f32 / total as f32) * 100.0 } else { 0.0 };
            println!("  {}: {} total, {} success, {} errors ({:.1}% success rate)", 
                device_ip, total, success, errors, success_rate);
        }
        println!("=============================\n");
    }
}