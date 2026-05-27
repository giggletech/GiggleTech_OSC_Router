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
use wry::http::Request;
use wry::WebViewBuilder;

use crate::config_editor;
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
  padding: 16px 20px;
  flex-shrink: 0;
  box-shadow: 0 2px 16px rgba(192, 38, 211, 0.4);
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.header-text h1 { font-size: 1.5rem; font-weight: 600; }
.header-text p { font-size: 0.85rem; opacity: 0.92; margin-top: 2px; }
.header-actions { display: flex; gap: 8px; flex-shrink: 0; }
.hdr-btn {
  padding: 9px 14px;
  font-size: 0.85rem;
  font-weight: 600;
  font-family: inherit;
  color: #7c3aed;
  background: #fff;
  border: none;
  border-radius: 8px;
  cursor: pointer;
}
.hdr-btn:hover { background: #f3e8ff; }
.hdr-btn.active { background: #ede9fe; box-shadow: inset 0 0 0 2px #5b21b6; }
#main { flex: 1; min-height: 0; display: flex; flex-direction: column; overflow: hidden; }
.panel { display: none; flex: 1; min-height: 0; flex-direction: column; overflow: hidden; }
.panel.visible { display: flex; }
#home-view {
  align-items: center;
  justify-content: center;
  padding: 32px;
  color: #8888a0;
  text-align: center;
  gap: 8px;
}
#log-wrap { padding: 14px; }
#log {
  flex: 1;
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
  min-height: 0;
}
#config-wrap { padding: 14px; gap: 12px; }
#config-status {
  font-size: 0.85rem;
  padding: 8px 12px;
  border-radius: 8px;
  display: none;
}
#config-status.ok { display: block; background: #14532d; color: #86efac; }
#config-status.err { display: block; background: #450a0a; color: #fca5a5; }
#device-list { flex: 1; overflow-y: auto; display: flex; flex-direction: column; gap: 10px; min-height: 0; }
.device-card {
  background: #16161e;
  border: 1px solid #2a2a36;
  border-radius: 10px;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.device-card h3 { font-size: 0.9rem; color: #c4b5fd; }
.device-card label { display: flex; flex-direction: column; gap: 4px; font-size: 0.8rem; color: #a1a1b5; }
.device-card input {
  padding: 8px 10px;
  border-radius: 6px;
  border: 1px solid #3f3f4e;
  background: #0f0f14;
  color: #e8e8f0;
  font-size: 0.9rem;
}
.device-card input:focus { outline: none; border-color: #a855f7; }
.btn-row { display: flex; gap: 8px; flex-wrap: wrap; }
.btn {
  padding: 9px 14px;
  font-size: 0.85rem;
  font-weight: 600;
  font-family: inherit;
  border: none;
  border-radius: 8px;
  cursor: pointer;
}
.btn-primary { background: #7c3aed; color: #fff; }
.btn-primary:hover { background: #6d28d9; }
.btn-secondary { background: #2a2a36; color: #e8e8f0; }
.btn-secondary:hover { background: #3f3f4e; }
.btn-danger { background: #450a0a; color: #fca5a5; }
.btn-danger:hover { background: #7f1d1d; }
.hint { font-size: 0.8rem; color: #6b6b80; margin-top: 4px; }
</style>
</head>
<body>
<header>
  <div class="header-text">
    <h1>GiggleTech</h1>
    <p>OSC Router</p>
  </div>
  <div class="header-actions">
    <button type="button" class="hdr-btn" id="show-logs-btn" onclick="showPanel('logs')">Output</button>
    <button type="button" class="hdr-btn" id="show-config-btn" onclick="showPanel('config')">Config</button>
  </div>
</header>
<div id="main">
  <div id="home-view" class="panel visible">
    <p>Router is running.</p>
    <p class="hint">Use <strong>Output</strong> for logs or <strong>Config</strong> to edit devices.</p>
  </div>
  <div id="log-wrap" class="panel"><pre id="log"></pre></div>
  <div id="config-wrap" class="panel">
    <div id="config-status"></div>
    <div id="device-list"></div>
    <div class="btn-row">
      <button type="button" class="btn btn-secondary" onclick="addDevice()">+ Add Device</button>
      <button type="button" class="btn btn-primary" onclick="saveConfig()">Save config.yml</button>
    </div>
    <p class="hint">Saving reloads the router automatically.</p>
  </div>
</div>
<script>
let activePanel = 'home';
let editorDevices = [];

function showPanel(name) {
  activePanel = name;
  document.getElementById('home-view').classList.toggle('visible', name === 'home');
  document.getElementById('log-wrap').classList.toggle('visible', name === 'logs');
  document.getElementById('config-wrap').classList.toggle('visible', name === 'config');
  document.getElementById('show-logs-btn').classList.toggle('active', name === 'logs');
  document.getElementById('show-config-btn').classList.toggle('active', name === 'config');
  if (name === 'config') {
    window.ipc.postMessage('load-config');
  }
}

function setConfigStatus(msg, isError) {
  const el = document.getElementById('config-status');
  el.textContent = msg;
  el.className = isError ? 'err' : 'ok';
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function renderDevices() {
  const list = document.getElementById('device-list');
  if (!editorDevices.length) {
    list.innerHTML = '<p class="hint">No devices yet. Click Add Device.</p>';
    return;
  }
  list.innerHTML = editorDevices.map((d, i) => `
    <div class="device-card">
      <h3>Device ${i + 1}</h3>
      <label>IP address
        <input type="text" value="${escapeHtml(d.ip)}" oninput="editorDevices[${i}].ip=this.value">
      </label>
      <label>Proximity parameter
        <input type="text" value="${escapeHtml(d.proximity_parameter)}" placeholder="proximity_01"
          oninput="editorDevices[${i}].proximity_parameter=this.value">
      </label>
      <button type="button" class="btn btn-danger" onclick="removeDevice(${i})">Remove</button>
    </div>
  `).join('');
}

function addDevice() {
  editorDevices.push({ ip: '', proximity_parameter: 'proximity_01' });
  renderDevices();
}

function removeDevice(index) {
  editorDevices.splice(index, 1);
  renderDevices();
}

function saveConfig() {
  setConfigStatus('Saving...', false);
  window.ipc.postMessage('save-config:' + JSON.stringify({ devices: editorDevices }));
}

window.onConfigLoaded = function(state) {
  editorDevices = state.devices || [];
  renderDevices();
  setConfigStatus('Loaded from config.yml', false);
};

window.onConfigSaved = function() {
  setConfigStatus('Saved to config.yml', false);
  window.ipc.postMessage('load-config');
};

window.onConfigError = function(msg) {
  setConfigStatus(msg, true);
};

function setLogs(lines) {
  const el = document.getElementById('log');
  el.textContent = lines.join('\n');
  if (activePanel === 'logs') el.scrollTop = el.scrollHeight;
}

function appendLog(line) {
  const el = document.getElementById('log');
  el.textContent += (el.textContent ? '\n' : '') + line;
  if (activePanel === 'logs') el.scrollTop = el.scrollHeight;
}
</script>
</body>
</html>"#;

enum UserEvent {
  TrayIconEvent(tray_icon::TrayIconEvent),
  MenuEvent(tray_icon::menu::MenuEvent),
  LogUpdated,
  ConfigIpc(String),
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

  fn create_output_window(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    ipc_proxy: tao::event_loop::EventLoopProxy<UserEvent>,
  ) {
    let window = WindowBuilder::new()
      .with_title("GiggleTech")
      .with_inner_size(LogicalSize::new(720.0, 520.0))
      .with_min_inner_size(LogicalSize::new(480.0, 320.0))
      .build(event_loop)
      .expect("Failed to create output window");

    let webview = WebViewBuilder::new()
      .with_html(OUTPUT_HTML)
      .with_ipc_handler(move |request: Request<String>| {
        let _ = ipc_proxy.send_event(UserEvent::ConfigIpc(request.body().clone()));
      })
      .build(&window)
      .expect("Failed to create output webview");

    self.logs_synced = 0;
    sync_logs_to_webview(&webview, &mut self.logs_synced);

    self.output = Some(OutputWindow { window, webview });
  }

  fn show_output(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    ipc_proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
  ) {
    if self.output.is_none() {
      self.create_output_window(event_loop, ipc_proxy.clone());
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

fn handle_config_ipc(webview: &wry::WebView, msg: &str) {
  if msg == "load-config" {
    match config_editor::load_editor_json() {
      Ok(json) => {
        let _ = webview.evaluate_script(&format!("window.onConfigLoaded({});", json));
      }
      Err(e) => {
        let err = serde_json::to_string(&e).unwrap_or_else(|_| "\"Unknown error\"".to_string());
        let _ = webview.evaluate_script(&format!("window.onConfigError({});", err));
      }
    }
  } else if let Some(json) = msg.strip_prefix("save-config:") {
    match config_editor::save_editor_json(json) {
      Ok(()) => {
        let _ = webview.evaluate_script("window.onConfigSaved();");
      }
      Err(e) => {
        let err = serde_json::to_string(&e).unwrap_or_else(|_| "\"Unknown error\"".to_string());
        let _ = webview.evaluate_script(&format!("window.onConfigError({});", err));
      }
    }
  }
}

/// Run the tray icon event loop on the main thread. Blocks until the user exits.
pub fn run() {
  let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
  let event_proxy = event_loop.create_proxy();
  let ipc_proxy = event_loop.create_proxy();

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

        ui_state.show_output(event_loop, &ipc_proxy);
      }

      Event::UserEvent(UserEvent::LogUpdated) => {
        ui_state.on_log_updated();
      }

      Event::UserEvent(UserEvent::ConfigIpc(msg)) => {
        if let Some(output) = &ui_state.output {
          handle_config_ipc(&output.webview, &msg);
        }
      }

      Event::UserEvent(UserEvent::MenuEvent(menu_event)) => {
        if menu_event.id == show_output.id() {
          ui_state.show_output(event_loop, &ipc_proxy);
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
            ui_state.show_output(event_loop, &ipc_proxy);
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
