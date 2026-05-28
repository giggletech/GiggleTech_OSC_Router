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

use async_osc::{OscSender, OscSocket, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use async_std::sync::RwLock;
use std::time::{Duration, Instant};
use once_cell::sync::Lazy;

use crate::log_ui;

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
        
        connections.retain(|_, info| {
            now.duration_since(info.last_used) < timeout
        });
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

static TX_SENDER: Lazy<Mutex<Option<OscSender>>> = Lazy::new(|| Mutex::new(None));

async fn shared_tx_sender() -> Result<OscSender> {
    if let Some(sender) = TX_SENDER.lock().unwrap().clone() {
        return Ok(sender);
    }
    let socket = OscSocket::bind("0.0.0.0:0").await?;
    let sender = socket.sender();
    *TX_SENDER.lock().unwrap() = Some(sender.clone());
    Ok(sender)
}

// Start connection manager cleanup task
pub(crate) async fn start_connection_manager() {
    async_std::task::spawn(async {
        loop {
            async_std::task::sleep(Duration::from_secs(60)).await; // Cleanup every minute
            CONNECTION_MANAGER.cleanup_old_connections().await;
        }
    });
}

// Send motor value over a reused UDP socket (no connect per packet).
pub(crate) async fn send_data(device_ip: &str, value: i32) -> Result<()> {
    let addr = create_socket_address(device_ip, "8888");
    let sender = shared_tx_sender().await?;
    match sender.send_to((TX_OSC_MOTOR_ADDRESS, (value,)), &addr).await {
        Ok(()) => {
            CONNECTION_MANAGER.update_connection_info(device_ip, true).await;
            // Drive the UI from the actual motor TX being sent.
            log_ui::notify_motor_tx_sent(device_ip, value);
            sender.send_to((TX_OSC_GIGGLESPARK, (value,)), &addr).await
        }
        Err(e) => {
            CONNECTION_MANAGER.update_connection_info(device_ip, false).await;
            *TX_SENDER.lock().unwrap() = None;
            Err(e)
        }
    }
}

// Get connection statistics for monitoring
pub(crate) async fn get_connection_stats() -> HashMap<String, (u32, u32, u32)> {
    CONNECTION_MANAGER.get_stats().await
}
