//! System tray UI, output window, auto-start toggle, exit.

use std::io;
use std::path::PathBuf;

use tao::{
  dpi::LogicalSize,
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget},
  window::{Window, WindowBuilder},
};
use tray_icon::{
  menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  TrayIconBuilder, TrayIconEvent,
};
use winreg::enums::*;
use winreg::RegKey;
use wry::WebViewBuilder;

use crate::log_ui;

const AUTO_START_VALUE_NAME: &str = "GiggleTechOSCRouter";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

const OUTPUT_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { height: 100%; }
body {
  background: #0f0f14;
  color: #e8e8f0;
  font-family: "Segoe UI", system-ui, sans-serif;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
header {
  background: linear-gradient(135deg, #c026d3 0%, #7c3aed 100%);
  padding: 18px 22px;
  flex-shrink: 0;
  box-shadow: 0 2px 16px rgba(192, 38, 211, 0.4);
}
header h1 {
  font-size: 1.65rem;
  font-weight: 600;
  letter-spacing: 0.03em;
}
header p {
  font-size: 0.85rem;
  opacity: 0.92;
  margin-top: 4px;
}
#log-wrap {
  flex: 1;
  min-height: 0;
  padding: 14px;
}
#log {
  height: 100%;
  overflow-y: auto;
  font-family: "Cascadia Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  padding: 14px;
  background: #16161e;
  border-radius: 10px;
  border: 1px solid #2a2a36;
  white-space: pre-wrap;
  word-break: break-word;
}
#log::-webkit-scrollbar { width: 8px; }
#log::-webkit-scrollbar-thumb { background: #4a4a5c; border-radius: 4px; }
</style>
</head>
<body>
<header>
  <h1>GiggleTech</h1>
  <p>OSC Router</p>
</header>
<div id="log-wrap"><pre id="log"></pre></div>
<script>
function setLogs(lines) {
  const el = document.getElementById('log');
  el.textContent = lines.join('\n');
  el.scrollTop = el.scrollHeight;
}
function appendLog(line) {
  const el = document.getElementById('log');
  el.textContent += (el.textContent ? '\n' : '') + line;
  el.scrollTop = el.scrollHeight;
}
</script>
</body>
</html>"#;

enum UserEvent {
  TrayIconEvent(tray_icon::TrayIconEvent),
  MenuEvent(tray_icon::menu::MenuEvent),
  LogUpdated,
}

struct OutputWindow {
  window: Window,
  webview: wry::WebView,
}

struct UiState {
  output: Option<OutputWindow>,
  logs_synced: usize,
}

impl UiState {
  fn new() -> Self {
    Self {
      output: None,
      logs_synced: 0,
    }
  }

  fn output_window_id(&self) -> Option<tao::window::WindowId> {
    self.output.as_ref().map(|o| o.window.id())
  }

  fn create_output_window(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) {
    let window = WindowBuilder::new()
      .with_title("GiggleTech")
      .with_inner_size(LogicalSize::new(720.0, 520.0))
      .build(event_loop)
      .expect("Failed to create output window");

    let webview = WebViewBuilder::new()
      .with_html(OUTPUT_HTML)
      .build(&window)
      .expect("Failed to create output webview");

    self.logs_synced = 0;
    sync_logs_to_webview(&webview, &mut self.logs_synced);

    self.output = Some(OutputWindow { window, webview });
  }

  fn show_output(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) {
    if self.output.is_none() {
      self.create_output_window(event_loop);
    }

    if let Some(output) = &self.output {
      output.window.set_visible(true);
      output.window.set_focus();
      sync_logs_to_webview(&output.webview, &mut self.logs_synced);
    }
  }

  fn hide_output(&mut self) {
    if let Some(output) = &self.output {
      output.window.set_visible(false);
    }
  }

  fn is_output_visible(&self) -> bool {
    self
      .output
      .as_ref()
      .map(|o| o.window.is_visible())
      .unwrap_or(false)
  }

  fn on_log_updated(&mut self) {
    if !self.is_output_visible() {
      return;
    }
    if let Some(output) = &self.output {
      push_new_logs(&output.webview, &mut self.logs_synced);
    }
  }

  fn close_output(&mut self) {
    self.output = None;
    self.logs_synced = 0;
  }
}

fn sync_logs_to_webview(webview: &wry::WebView, synced: &mut usize) {
  let lines = log_ui::snapshot();
  *synced = lines.len();
  if let Ok(json) = serde_json::to_string(&lines) {
    let _ = webview.evaluate_script(&format!("setLogs({});", json));
  }
}

fn push_new_logs(webview: &wry::WebView, synced: &mut usize) {
  let lines = log_ui::snapshot();
  if lines.len() <= *synced {
    return;
  }
  for line in &lines[*synced..] {
    if let Ok(json) = serde_json::to_string(line) {
      let _ = webview.evaluate_script(&format!("appendLog({});", json));
    }
  }
  *synced = lines.len();
}

/// Run the tray icon event loop on the main thread. Blocks until the user exits.
pub fn run() {
  let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
  let event_proxy = event_loop.create_proxy();

  log_ui::set_log_notify(move || {
    let _ = event_proxy.send_event(UserEvent::LogUpdated);
  });

  let proxy = event_loop.create_proxy();
  TrayIconEvent::set_event_handler(Some(move |event| {
    let _ = proxy.send_event(UserEvent::TrayIconEvent(event));
  }));

  let proxy = event_loop.create_proxy();
  MenuEvent::set_event_handler(Some(move |event| {
    let _ = proxy.send_event(UserEvent::MenuEvent(event));
  }));

  let tray_menu = Menu::new();

  let show_output = MenuItem::new("Show Output", true, None);
  let hide_output = MenuItem::new("Hide Output", true, None);
  let auto_start = CheckMenuItem::with_id(
    "start_with_windows",
    "Start with Windows",
    true,
    is_auto_start_enabled(),
    None,
  );
  let exit_item = MenuItem::new("Exit", true, None);

  let _ = tray_menu.append_items(&[
    &show_output,
    &hide_output,
    &PredefinedMenuItem::separator(),
    &auto_start,
    &PredefinedMenuItem::separator(),
    &exit_item,
  ]);

  let mut tray_icon = None;
  let mut ui_state = UiState::new();

  event_loop.run(move |event, event_loop, control_flow| {
    *control_flow = ControlFlow::Wait;

    match event {
      Event::NewEvents(tao::event::StartCause::Init) => {
        let icon = create_tray_icon();
        tray_icon = Some(
          TrayIconBuilder::new()
            .with_menu(Box::new(tray_menu.clone()))
            .with_tooltip("GiggleTech OSC Router")
            .with_icon(icon)
            .build()
            .expect("Failed to create tray icon"),
        );

        ui_state.show_output(event_loop);
      }

      Event::UserEvent(UserEvent::LogUpdated) => {
        ui_state.on_log_updated();
      }

      Event::UserEvent(UserEvent::MenuEvent(menu_event)) => {
        if menu_event.id == show_output.id() {
          ui_state.show_output(event_loop);
        } else if menu_event.id == hide_output.id() {
          ui_state.hide_output();
        } else if menu_event.id == auto_start.id() {
          let new_checked = !auto_start.is_checked();
          auto_start.set_checked(new_checked);
          if let Err(e) = set_auto_start(new_checked) {
            log_ui::app_log(&format!("Failed to update auto-start: {}", e));
            auto_start.set_checked(!new_checked);
          }
        } else if menu_event.id == exit_item.id() {
          tray_icon.take();
          ui_state.close_output();
          *control_flow = ControlFlow::Exit;
        }
      }

      Event::UserEvent(UserEvent::TrayIconEvent(tray_event)) => {
        if let tray_icon::TrayIconEvent::Click { button, .. } = tray_event {
          if button == tray_icon::MouseButton::Left {
            ui_state.show_output(event_loop);
          }
        }
      }

      Event::WindowEvent {
        window_id,
        event: WindowEvent::CloseRequested,
        ..
      } if Some(window_id) == ui_state.output_window_id() => {
        ui_state.hide_output();
      }

      _ => {}
    }
  });
}

fn create_tray_icon() -> tray_icon::Icon {
  let width = 16u32;
  let height = 16u32;
  let mut rgba = vec![0u8; (width * height * 4) as usize];
  let center_x = 7.5f64;
  let center_y = 7.5f64;
  let radius = 6.0f64;

  for y in 0..height {
    for x in 0..width {
      let dx = x as f64 - center_x;
      let dy = y as f64 - center_y;
      let idx = ((y * width + x) * 4) as usize;
      if dx * dx + dy * dy <= radius * radius {
        rgba[idx] = 255;
        rgba[idx + 1] = 0;
        rgba[idx + 2] = 255;
        rgba[idx + 3] = 255;
      }
    }
  }

  tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon image")
}

fn is_auto_start_enabled() -> bool {
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  match hkcu.open_subkey(RUN_KEY) {
    Ok(run) => run.get_value::<String, _>(AUTO_START_VALUE_NAME).is_ok(),
    Err(_) => false,
  }
}

fn set_auto_start(enabled: bool) -> io::Result<()> {
  let hkcu = RegKey::predef(HKEY_CURRENT_USER);
  let (run, _) = hkcu.create_subkey(RUN_KEY)?;

  if enabled {
    let exe = current_exe_path()?;
    let quoted = format!("\"{}\"", exe.display());
    run.set_value(AUTO_START_VALUE_NAME, &quoted)?;
  } else {
    let _ = run.delete_value(AUTO_START_VALUE_NAME);
  }

  Ok(())
}

fn current_exe_path() -> io::Result<PathBuf> {
  std::env::current_exe()
}
