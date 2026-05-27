//! Shared log buffer for the output window and giggletech_log.txt.



use std::fs::OpenOptions;

use std::io::Write;

use std::sync::atomic::{AtomicBool, Ordering};

use std::sync::{Arc, Mutex};



use chrono::Local;

use once_cell::sync::OnceCell;



// In-memory tail for the output window; UI shows only what fits in the card.

const MAX_LINES: usize = 100;



static LOG_LINES: once_cell::sync::Lazy<Arc<Mutex<Vec<String>>>> =

  once_cell::sync::Lazy::new(|| Arc::new(Mutex::new(Vec::new())));



static CONSOLE_MIRROR: AtomicBool = AtomicBool::new(true);



static LOG_NOTIFY: OnceCell<Box<dyn Fn() + Send + Sync>> = OnceCell::new();



static PROXIMITY_NOTIFY: OnceCell<Box<dyn Fn(&str, f32) + Send + Sync>> = OnceCell::new();



/// When true, log lines are also printed to stdout (for `--no-tray` mode).

pub fn set_console_mirror(enabled: bool) {

  CONSOLE_MIRROR.store(enabled, Ordering::Relaxed);

}



/// Register a callback invoked when new log lines are written (e.g. to refresh the UI).

pub fn set_log_notify(notify: impl Fn() + Send + Sync + 'static) {

  let _ = LOG_NOTIFY.set(Box::new(notify));

}



/// Register a callback invoked when live proximity changes (drives motor bars directly).

pub fn set_proximity_notify(notify: impl Fn(&str, f32) + Send + Sync + 'static) {

  let _ = PROXIMITY_NOTIFY.set(Box::new(notify));

}



/// Push a proximity value to the output window motor bar for `parameter`.

pub fn notify_proximity(parameter: &str, value: f32) {

  if let Some(notify) = PROXIMITY_NOTIFY.get() {

    let key = parameter.trim_start_matches("/avatar/parameters/");

    notify(key, value);

  }

}



/// Status / errors: shown in the output window and appended to `giggletech_log.txt`.

pub fn app_log(message: &str) {

  let now = Local::now();

  let line = format!("[{}] {}", now.format("%Y-%m-%d %H:%M:%S"), message);

  push_line(&line, true);

}



/// Live feedback (pat bars, startup banner): output window only, not written to the log file.

pub fn ui_line(message: &str) {

  push_line(message, false);

}



/// Alias for [`ui_line`] (display-only).

pub fn log_line(message: &str) {

  ui_line(message);

}



fn push_line(line: &str, persist_to_file: bool) {
  if let Ok(mut lines) = LOG_LINES.lock() {
    lines.push(line.to_string());
    if lines.len() > MAX_LINES {
      let excess = lines.len() - MAX_LINES;
      lines.drain(0..excess);
    }
  }

  if persist_to_file {
    write_to_file(line);
  }

  maybe_println(line);

  if let Some(notify) = LOG_NOTIFY.get() {
    notify();
  }
}



fn write_to_file(line: &str) {

  match OpenOptions::new()

    .create(true)

    .append(true)

    .open("giggletech_log.txt")

  {

    Ok(mut file) => {

      if let Err(e) = writeln!(file, "{}", line) {

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


