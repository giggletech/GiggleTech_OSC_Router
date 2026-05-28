//! ICMP reachability checks for configured device IPs.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct PingResult {
  pub ip: String,
  pub online: bool,
}

/// Ping a host once (Windows: `ping -n 1 -w 1000`). Does not write to the UI log.
pub async fn ping_host(device_ip: &str) -> bool {
  let device_ip = device_ip.trim();
  if device_ip.is_empty() {
    return false;
  }

  #[cfg(windows)]
  {
    // Since the app runs as a GUI subsystem process in release builds,
    // spawning `ping.exe` (a console program) can pop up a new console window.
    // Run it with CREATE_NO_WINDOW so pings are always silent.
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let ip = device_ip.to_string();
    return async_std::task::spawn_blocking(move || {
      use std::os::windows::process::CommandExt;
      std::process::Command::new("ping")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-n", "1", "-w", "1000", &ip])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    })
    .await;
  }

  #[cfg(not(windows))]
  {
    match async_std::process::Command::new("ping")
      .args(["-c", "1", "-W", "1", device_ip])
      .output()
      .await
    {
      Ok(output) => output.status.success(),
      Err(_) => false,
    }
  }
}

pub async fn ping_hosts(ips: &[String]) -> Vec<PingResult> {
  let mut results = Vec::new();
  for ip in ips {
    let ip = ip.trim();
    if ip.is_empty() {
      continue;
    }
    let online = ping_host(ip).await;
    results.push(PingResult {
      ip: ip.to_string(),
      online,
    });
  }
  results
}
