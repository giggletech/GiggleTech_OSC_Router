//! mDNS lookup for GiggleTech device web config (e.g. giggletech.local).

use std::time::{Duration, Instant};

use mdns_sd::{
  HostnameResolutionEvent, ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent,
};
use serde::Serialize;

/// User-facing hostname (browser / config UI).
pub const GIGGLETECH_HOSTNAME: &str = "giggletech.local";
/// mDNS wire format — mdns-sd requires a trailing dot after `.local`.
const GIGGLETECH_MDNS_HOSTNAME: &str = "giggletech.local.";
const HTTP_SERVICE: &str = "_http._tcp.local.";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Serialize)]
pub struct MdnsLookupResult {
  pub device_index: usize,
  pub found: bool,
  pub hostname: Option<String>,
  pub ip: Option<String>,
  pub url: Option<String>,
  pub message: String,
}

fn scoped_ip_to_string(ip: &ScopedIp) -> Option<String> {
  match ip {
    ScopedIp::V4(v4) => Some(v4.addr().to_string()),
    ScopedIp::V6(v6) => Some(v6.addr().to_string()),
    _ => None,
  }
}

fn pick_ip_from_service(service: &ResolvedService) -> Option<String> {
  service
    .addresses
    .iter()
    .find_map(scoped_ip_to_string)
}

fn normalize_hostname(host: &str) -> String {
  host.trim().trim_end_matches('.').to_string()
}

fn is_giggletech_service(service: &ResolvedService) -> bool {
  let host = normalize_hostname(&service.host).to_ascii_lowercase();
  let name = service.fullname.to_ascii_lowercase();
  host.contains("giggletech") || name.contains("giggletech")
}

fn result_from_service(device_index: usize, service: &ResolvedService) -> MdnsLookupResult {
  let hostname = normalize_hostname(&service.host);
  let ip = pick_ip_from_service(service);
  let url = Some(format!("http://{}", hostname));
  MdnsLookupResult {
    device_index,
    found: true,
    hostname: Some(hostname.clone()),
    ip: ip.clone(),
    url,
    message: match ip {
      Some(ip) => format!("Found {} ({})", hostname, ip),
      None => format!("Found {} (no IP yet)", hostname),
    },
  }
}

fn result_from_hostname(device_index: usize, hostname: &str, ips: &[ScopedIp]) -> MdnsLookupResult {
  let hostname = normalize_hostname(hostname);
  let ip = ips.iter().find_map(scoped_ip_to_string);
  let url = Some(format!("http://{}", hostname));
  MdnsLookupResult {
    device_index,
    found: ip.is_some(),
    hostname: Some(hostname.clone()),
    ip: ip.clone(),
    url,
    message: match ip {
      Some(ip) => format!("Found {} ({})", hostname, ip),
      None => format!("Found {} (no IP yet)", hostname),
    },
  }
}

fn not_found(device_index: usize) -> MdnsLookupResult {
  MdnsLookupResult {
    device_index,
    found: false,
    hostname: None,
    ip: None,
    url: None,
    message: "No GiggleTech device found on the network.".to_string(),
  }
}

/// Browse mDNS for the GiggleTech web config hostname and IP.
pub fn lookup_giggletech_webpage(device_index: usize) -> MdnsLookupResult {
  let mdns = match ServiceDaemon::new() {
    Ok(daemon) => daemon,
    Err(e) => {
      return MdnsLookupResult {
        device_index,
        found: false,
        hostname: None,
        ip: None,
        url: None,
        message: format!("mDNS unavailable: {}", e),
      };
    }
  };

  let browse_rx = match mdns.browse(HTTP_SERVICE) {
    Ok(rx) => rx,
    Err(e) => {
      let _ = mdns.shutdown();
      return MdnsLookupResult {
        device_index,
        found: false,
        hostname: None,
        ip: None,
        url: None,
        message: format!("mDNS browse failed: {}", e),
      };
    }
  };

  let host_rx = mdns
    .resolve_hostname(GIGGLETECH_MDNS_HOSTNAME, Some(DISCOVERY_TIMEOUT.as_secs()))
    .ok();

  let deadline = Instant::now() + DISCOVERY_TIMEOUT;
  let mut service_match: Option<ResolvedService> = None;
  let mut hostname_ips: Vec<ScopedIp> = Vec::new();

  while Instant::now() < deadline {
    let remaining = deadline.saturating_duration_since(Instant::now());
    let wait = remaining.min(Duration::from_millis(250));

    if let Ok(event) = browse_rx.recv_timeout(wait) {
      if let ServiceEvent::ServiceResolved(service) = event {
        if is_giggletech_service(&service) && pick_ip_from_service(&service).is_some() {
          service_match = Some(*service);
          break;
        }
      }
    }

    if service_match.is_none() {
      if let Some(host_rx) = host_rx.as_ref() {
        while let Ok(event) = host_rx.try_recv() {
          match event {
            HostnameResolutionEvent::AddressesFound(_, ips) => {
              hostname_ips.extend(ips);
            }
            HostnameResolutionEvent::SearchTimeout(_)
            | HostnameResolutionEvent::SearchStopped(_) => {
              break;
            }
            _ => {}
          }
        }
      }
      if !hostname_ips.is_empty() {
        break;
      }
    }
  }

  let _ = mdns.shutdown();

  if let Some(service) = service_match {
    return result_from_service(device_index, &service);
  }

  if !hostname_ips.is_empty() {
    return result_from_hostname(device_index, GIGGLETECH_HOSTNAME, &hostname_ips);
  }

  not_found(device_index)
}
