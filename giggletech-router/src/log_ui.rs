//! Shared log buffer for the output window and giggletech_log.txt.

use std::fs::OpenOptions;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Local;
use once_cell::sync::OnceCell;

const MAX_LINES: usize = 2000;

static LOG_LINES: once_cell::sync::Lazy<Arc<Mutex<Vec<String>>>> =
  once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));

static CONSOLE_MIRROR: AtomicBool = AtomicBool::new(true);

static LOG_NOTIFY: OnceCell<Box<dyn Fn() + Send + Sync>> = OnceCell::new();

/// When true, log lines are also printed to stdout (for `--no-tray` mode).
pub fn set_console_mirror(enabled: bool) {
  CONSOLE_MIRROR.store(enabled, Ordering::Relaxed);
}

/// Register a callback invoked when new log lines are written (e.g. to refresh the UI).
pub fn set_log_notify(notify: impl Fn() + Send + Sync + 'static) {
  let _ = LOG_NOTIFY.set(Box::new(notify));
}

/// Log with a timestamp (status messages, errors, events).
pub fn app_log(message: &str) {
  let now = Local::now();
  let line = format!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), message);
  push_line(&line, true);
}

/// Log a raw line (banner art, motor graphs) without an extra timestamp prefix.
pub fn log_line(message: &str) {
  push_line(message, false);
}

fn push_line(line: &str, write_timestamped_file: bool) {
  if let Ok(mut lines) = LOG_LINES.lock() {
    lines.push(line.to_string());
    if lines.len() > MAX_LINES {
      let excess = lines.len() - MAX_LINES;
      lines.drain(0..excess);
    }
  }

  write_to_file(line, write_timestamped_file);
  maybe_println(line);

  if let Some(notify) = LOG_NOTIFY.get() {
    notify();
  }
}

fn write_to_file(line: &str, already_timestamped: bool) {
  let file_line = if already_timestamped {
    line.to_string()
  } else {
    let now = Local::now();
    format!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), line)
  };

  match OpenOptions::new()
    .create(true)
    .append(true)
    .open("giggletech_log.txt")
  {
    Ok(mut file) => {
      if let Err(e) = writeln!(file, "{}", file_line) {
        eprintln!("Failed to write to log file: {}", e);
      }
    }
    Err(e) => eprintln!("Failed to open log file: {}", e),
  }
}

fn maybe_println(line: &str) {
  if CONSOLE_MIRROR.load(Ordering::Relaxed) {
    println!("{}", line);
  }
}

pub fn snapshot() -> Vec<String> {
  LOG_LINES
    .lock()
    .map(|lines| lines.clone())
    .unwrap_or_default()
}
