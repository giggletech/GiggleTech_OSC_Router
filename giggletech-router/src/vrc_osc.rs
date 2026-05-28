//! Send OSC messages to VRChat (typically 127.0.0.1:9000).

use async_osc::{OscSender, OscSocket, Result};
use once_cell::sync::Lazy;
use std::sync::Mutex;

use crate::giggletech_osc::create_socket_address;

const VRC_HOST: &str = "127.0.0.1";
const VRC_PORT: &str = "9000";

static VRC_SENDER: Lazy<Mutex<Option<OscSender>>> = Lazy::new(|| Mutex::new(None));

async fn shared_sender() -> Result<OscSender> {
  if let Some(sender) = VRC_SENDER.lock().unwrap().clone() {
    return Ok(sender);
  }
  let socket = OscSocket::bind("0.0.0.0:0").await?;
  let sender = socket.sender();
  *VRC_SENDER.lock().unwrap() = Some(sender.clone());
  Ok(sender)
}

/// Send a bool value to a VRChat avatar parameter address.
pub async fn send_avatar_parameter(address: &str, value: bool) -> Result<()> {
  let addr = create_socket_address(VRC_HOST, VRC_PORT);
  let sender = shared_sender().await?;
  match sender.send_to((address, (value,)), &addr).await {
    Ok(()) => Ok(()),
    Err(e) => {
      *VRC_SENDER.lock().unwrap() = None;
      Err(e)
    }
  }
}

