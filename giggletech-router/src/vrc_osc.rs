//! Send OSC messages to VRChat (typically 127.0.0.1:9000).

use async_osc::{OscSender, OscSocket, Result};
use async_std::task;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::time::Duration;

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

async fn send_bool(address: &str, value: bool) -> Result<()> {
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

/// Send online/offline to a VRChat avatar parameter as OSC bool.
///
/// When `pulse_on_online` is true, sends false then true so VRChat sees a fresh transition.
pub async fn send_avatar_parameter(
  address: &str,
  online: bool,
  pulse_on_online: bool,
) -> Result<()> {
  if online && pulse_on_online {
    send_bool(address, false).await?;
    task::sleep(Duration::from_millis(50)).await;
  }
  send_bool(address, online).await
}
