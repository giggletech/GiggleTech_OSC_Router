//! ICMP reachability checks for configured device IPs.
//!
//! A single background loop pings each registered IP once per interval; all consumers
//! (tray UI, router online monitor, startup checks) read from the shared cache.

use once_cell::sync::Lazy;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::process::Output;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static MONITOR: Lazy<Arc<DevicePingMonitor>> =
  Lazy::new(|| Arc::new(DevicePingMonitor::new(5000)));

pub fn monitor() -> Arc<DevicePingMonitor> {
  MONITOR.clone()
}

#[derive(Debug, Clone, Serialize)]
pub struct PingResult {
  pub ip: String,
  pub online: bool,
  /// False until this IP has been pinged at least once since registration.
  pub known: bool,
}

pub struct DevicePingMonitor {
  ips: Mutex<HashSet<String>>,
  cache: Mutex<HashMap<String, bool>>,
  interval_ms: AtomicU64,
  started: AtomicBool,
}

impl DevicePingMonitor {
  pub fn new(interval_ms: u64) -> Self {
    Self {
      ips: Mutex::new(HashSet::new()),
      cache: Mutex::new(HashMap::new()),
      interval_ms: AtomicU64::new(interval_ms),
      started: AtomicBool::new(false),
    }
  }

  pub fn set_interval_ms(&self, interval_ms: u64) {
    self.interval_ms.store(interval_ms.max(1000), Ordering::Relaxed);
  }

  /// Replace the registered IP set (deduped) and ensure the background loop is running.
  pub fn sync_ips(&self, ips: impl IntoIterator<Item = impl AsRef<str>>) {
    let mut set = HashSet::new();
    for ip in ips {
      let ip = ip.as_ref().trim();
      if !ip.is_empty() {
        set.insert(ip.to_string());
      }
    }
    *self.ips.lock().unwrap() = set.clone();
    self.cache.lock().unwrap().retain(|ip, _| set.contains(ip));
    if self
      .started
      .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
      .is_ok()
    {
      let this = monitor();
      async_std::task::spawn(async move {
        this.run_loop().await;
      });
    }
  }

  pub fn get(&self, ip: &str) -> Option<bool> {
    self.cache.lock().unwrap().get(ip.trim()).copied()
  }

  /// One entry per unique IP from `ips`, in order of first appearance.
  pub fn snapshot_for_ips(&self, ips: &[String]) -> Vec<PingResult> {
    let cache = self.cache.lock().unwrap();
    let mut seen = HashSet::new();
    let mut results = Vec::new();
    for ip in ips {
      let ip = ip.trim();
      if ip.is_empty() || !seen.insert(ip.to_string()) {
        continue;
      }
      if let Some(&online) = cache.get(ip) {
        results.push(PingResult {
          ip: ip.to_string(),
          online,
          known: true,
        });
      } else {
        results.push(PingResult {
          ip: ip.to_string(),
          online: false,
          known: false,
        });
      }
    }
    results
  }

  async fn run_loop(self: Arc<Self>) {
    loop {
      let ips: Vec<String> = self.ips.lock().unwrap().iter().cloned().collect();
      for ip in ips {
        let online = ping_host(&ip).await;
        self.cache.lock().unwrap().insert(ip, online);
      }
      let interval_ms = self.interval_ms.load(Ordering::Relaxed);
      async_std::task::sleep(Duration::from_millis(interval_ms)).await;
    }
  }
}

fn ping_stdout_indicates_failure(stdout: &[u8]) -> bool {
  let text = String::from_utf8_lossy(stdout).to_lowercase();
  text.contains("request timed out")
    || text.contains("destination host unreachable")
    || text.contains("could not find host")
    || text.contains("general failure")
}

/// Trust ping exit code; only override when stdout clearly says the host is down.
fn ping_output_online(output: &Output) -> bool {
  output.status.success() && !ping_stdout_indicates_failure(&output.stdout)
}

/// Ping a host once (Windows: `ping -n 1 -w 1000`). Does not write to the UI log.
async fn ping_host(device_ip: &str) -> bool {
  let device_ip = device_ip.trim();
  if device_ip.is_empty() {
    return false;
  }

  #[cfg(windows)]
  {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    let ip = device_ip.to_string();
    return match async_std::task::spawn_blocking(move || {
      use std::os::windows::process::CommandExt;
      std::process::Command::new("ping")
        .creation_flags(CREATE_NO_WINDOW)
        .args(["-n", "1", "-w", "1000", &ip])
        .output()
    })
    .await
    {
      Ok(output) => ping_output_online(&output),
      Err(_) => false,
    };
  }

  #[cfg(not(windows))]
  {
    match async_std::process::Command::new("ping")
      .args(["-c", "1", "-W", "1", device_ip])
      .output()
      .await
    {
      Ok(output) => ping_output_online(&output),
      Err(_) => false,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::ping_stdout_indicates_failure;

  #[test]
  fn good_echo_is_not_failure() {
    let out = b"Reply from 192.168.1.69: bytes=32 time=1ms TTL=64\r\n";
    assert!(!ping_stdout_indicates_failure(out));
  }

  #[test]
  fn unreachable_is_failure() {
    let out = b"Reply from 192.168.1.1: Destination host unreachable.\r\n";
    assert!(ping_stdout_indicates_failure(out));
  }

  #[test]
  fn timeout_is_failure() {
    let out = b"Request timed out.\r\n";
    assert!(ping_stdout_indicates_failure(out));
  }
}
