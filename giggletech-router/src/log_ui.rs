//! In-memory status lines for the output window (live console, not a log file).

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Local;
use once_cell::sync::OnceCell;

const MAX_LINES: usize = 100;

static STATUS_LINES: once_cell::sync::Lazy<Arc<Mutex<Vec<String>>>> =
  once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

#[derive(Debug, Clone)]
struct MotorUiParams {
  proximity_parameter: String,
  max_tx: f32,
}

/// Device IP → motor UI mapping (parameter + max TX for normalization).
static MOTOR_UI_BY_IP: once_cell::sync::Lazy<Mutex<std::collections::HashMap<String, MotorUiParams>>> =
  once_cell::sync::Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

/// Incremented when the ring buffer drops lines from the front (UI must full-resync).
static BUFFER_EPOCH: AtomicUsize = AtomicUsize::new(0);

static CONSOLE_MIRROR: AtomicBool = AtomicBool::new(true);

static STATUS_NOTIFY: OnceCell<Box<dyn Fn() + Send + Sync>> = OnceCell::new();

static PROXIMITY_NOTIFY: OnceCell<Box<dyn Fn(&str, f32) + Send + Sync>> = OnceCell::new();

static PROX_SIGNAL_NOTIFY: OnceCell<Box<dyn Fn(&str, &str, f32) + Send + Sync>> = OnceCell::new();

static HEADPAT_TELEMETRY_NOTIFY: OnceCell<Box<dyn Fn(&str, &str, &str) + Send + Sync>> = OnceCell::new();

static PAT_BAR_NOTIFY: OnceCell<Box<dyn Fn(&str, &str) + Send + Sync>> = OnceCell::new();

/// When true, status lines are also printed to stdout (`--no-tray` mode).
pub fn set_console_mirror(enabled: bool) {
  CONSOLE_MIRROR.store(enabled, Ordering::Relaxed);
}

/// Register a callback when new status lines are written (refreshes the output window).
pub fn set_status_notify(notify: impl Fn() + Send + Sync + 'static) {
  let _ = STATUS_NOTIFY.set(Box::new(notify));
}

/// Register a callback for live motor bars in the output window (`key` = device IP).
pub fn set_proximity_notify(notify: impl Fn(&str, f32) + Send + Sync + 'static) {
  let _ = PROXIMITY_NOTIFY.set(Box::new(notify));
}

/// Raw proximity parameter samples (for collider visualization).
pub fn set_prox_signal_notify(notify: impl Fn(&str, &str, f32) + Send + Sync + 'static) {
  let _ = PROX_SIGNAL_NOTIFY.set(Box::new(notify));
}

pub fn notify_prox_signal(device_ip: &str, parameter: &str, value: f32) {
  if let Some(notify) = PROX_SIGNAL_NOTIFY.get() {
    notify(device_ip, parameter, value);
  }
}

/// Headpat pipeline samples for the collider viz (`json` = serialized telemetry).
pub fn set_headpat_telemetry_notify(notify: impl Fn(&str, &str, &str) + Send + Sync + 'static) {
  let _ = HEADPAT_TELEMETRY_NOTIFY.set(Box::new(notify));
}

pub fn notify_headpat_telemetry(device_ip: &str, parameter: &str, json: &str) {
  if let Some(notify) = HEADPAT_TELEMETRY_NOTIFY.get() {
    notify(device_ip, parameter, json);
  }
}

pub fn notify_proximity(parameter: &str, value: f32) {
  if let Some(notify) = PROXIMITY_NOTIFY.get() {
    let key = parameter.trim_start_matches("/avatar/parameters/");
    notify(key, value);
  }
}

/// Replace motor UI mapping from current config devices.
///
/// This enables the motor bar to reflect the *actual motor output being sent* (from `send_data`),
/// including stop/timeout paths that don't flow through proximity handling.
pub fn set_motor_ui_devices(entries: Vec<(String, String, f32)>) {
  if let Ok(mut map) = MOTOR_UI_BY_IP.lock() {
    map.clear();
    for (ip, proximity_parameter, max_tx) in entries {
      let max_tx = max_tx.max(1.0);
      map.insert(
        ip,
        MotorUiParams {
          proximity_parameter,
          max_tx,
        },
      );
    }
  }
}

/// Notify motor output based on a send-to-device TX value.
pub fn notify_motor_tx_sent(device_ip: &str, motor_tx: i32) {
  let params = MOTOR_UI_BY_IP
    .lock()
    .ok()
    .and_then(|m| m.get(device_ip).cloned());
  let Some(params) = params else { return; };
  let motor_out = (motor_tx as f32 / params.max_tx).clamp(0.0, 1.0);
  // Key live motor UI by device IP so bars never cross onto another card.
  notify_proximity(device_ip, motor_out);
}

/// Register a callback for live pat bars (`---->`), separate from the status console.
pub fn set_pat_bar_notify(notify: impl Fn(&str, &str) + Send + Sync + 'static) {
  let _ = PAT_BAR_NOTIFY.set(Box::new(notify));
}

/// Update the ASCII pat bar for `parameter` (empty string clears it).
pub fn notify_pat_bar(parameter: &str, graph: &str) {
  if let Some(notify) = PAT_BAR_NOTIFY.get() {
    let key = parameter.trim_start_matches("/avatar/parameters/");
    notify(key, graph);
  }
}

/// Append a line to the live status console (and optional stdout mirror).
pub fn status(message: &str) {
  push_line(message);
}

pub fn buffer_epoch() -> usize {
  BUFFER_EPOCH.load(Ordering::Acquire)
}

pub fn line_count() -> usize {
  STATUS_LINES
    .lock()
    .map(|lines| lines.len())
    .unwrap_or(0)
}

/// Lines appended since `from_index` (before the latest push), and the new line count.
pub fn lines_since(from_index: usize) -> (Vec<String>, usize) {
  STATUS_LINES
    .lock()
    .map(|lines| {
      let len = lines.len();
      let start = from_index.min(len);
      (lines[start..].to_vec(), len)
    })
    .unwrap_or_default()
}

pub fn snapshot() -> Vec<String> {
  STATUS_LINES
    .lock()
    .map(|lines| lines.clone())
    .unwrap_or_default()
}

fn timestamp_prefix() -> String {
  Local::now().format("[%H:%M:%S] ").to_string()
}

fn push_line(line: &str) {
  let line = format!("{}{}", timestamp_prefix(), line);
  if CONSOLE_MIRROR.load(Ordering::Relaxed) {
    println!("{}", line);
  }

  if let Ok(mut lines) = STATUS_LINES.lock() {
    lines.push(line);
    if lines.len() > MAX_LINES {
      let excess = lines.len() - MAX_LINES;
      lines.drain(0..excess);
      BUFFER_EPOCH.fetch_add(1, Ordering::Release);
    }
  }

  if let Some(notify) = STATUS_NOTIFY.get() {
    notify();
  }
}
