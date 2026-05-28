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

  match async_std::process::Command::new("ping")
    .args(["-n", "1", "-w", "1000", device_ip])
    .output()
    .await
  {
    Ok(output) => output.status.success(),
    Err(_) => false,
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
