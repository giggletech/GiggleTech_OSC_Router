//! System tray UI, output window, auto-start toggle, exit.

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use once_cell::sync::Lazy;

use tao::{
  dpi::LogicalSize,
  event::{Event, WindowEvent},
  event_loop::{ControlFlow, EventLoopBuilder, EventLoopWindowTarget},
  window::{Icon as TaoIcon, Window, WindowBuilder},
};
use tray_icon::{
  menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem},
  TrayIconBuilder, TrayIconEvent,
};
use winreg::enums::*;
use winreg::RegKey;
use dirs::data_local_dir;
use wry::http::Request;
use wry::{WebContext, WebViewBuilder};

use serde::Deserialize;
use tao::event_loop::EventLoopProxy;

use crate::collider_viz::{self, ColliderVizState};
use crate::config_editor;
use crate::device_discovery;
use crate::device_ping;
use crate::device_test;
use crate::log_ui;
#[cfg(windows)]
use crate::single_instance::PrimaryInstance;

#[derive(Debug, Deserialize)]
struct StartupHeightRequest {
  h: f64,
}

const AUTO_START_VALUE_NAME: &str = "GiggleTechOSCRouter";
/// Passed on the Run registry command so login starts tray-only (no output window).
pub const AUTOSTART_ARG: &str = "--autostart";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const LOGO_PLACEHOLDER: &str = "{{LOGO_URI}}";
const COLLIDER_VIZ_STYLES_PLACEHOLDER: &str = "{{COLLIDER_VIZ_STYLES}}";
const COLLIDER_VIZ_RUNTIME_PLACEHOLDER: &str = "{{COLLIDER_VIZ_RUNTIME}}";
const COLLIDER_VIZ_CARD_INNER_PLACEHOLDER: &str = "{{COLLIDER_VIZ_CARD_INNER}}";
const OUTPUT_WINDOW_MIN_WIDTH: f64 = 960.0;
/// Initial width matches minimum so the window opens as narrow as allowed.
const OUTPUT_WINDOW_WIDTH: f64 = OUTPUT_WINDOW_MIN_WIDTH;
/// Initial height before JS measures content (kept modest; refined on config load).
const OUTPUT_WINDOW_HEIGHT: f64 = 520.0;
const OUTPUT_WINDOW_MIN_HEIGHT: f64 = 480.0;
/// Cap one-time startup fit so a measurement glitch cannot spawn a huge window.
const OUTPUT_WINDOW_STARTUP_MAX_HEIGHT: f64 = 1250.0;
/// When the log column is visible, never shrink below this on startup.
const OUTPUT_WINDOW_STARTUP_CONSOLE_MIN_HEIGHT: f64 = 720.0;
fn clamp_startup_height(h: f64) -> f64 {
  h.max(OUTPUT_WINDOW_MIN_HEIGHT)
    .min(OUTPUT_WINDOW_STARTUP_MAX_HEIGHT)
}

static PENDING_MOTOR_BARS: Lazy<Mutex<HashMap<String, f32>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_PAT_BARS: Lazy<Mutex<HashMap<String, String>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_PROX_SIGNALS: Lazy<Mutex<HashMap<String, f32>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_HEADPAT_TELEMETRY: Lazy<Mutex<HashMap<String, String>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static LAST_HEADPAT_TELEMETRY: Lazy<Mutex<HashMap<String, String>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static LIVE_UI_FLUSH_PENDING: AtomicBool = AtomicBool::new(false);

fn queue_live_ui_flush(proxy: &EventLoopProxy<UserEvent>) {
  if LIVE_UI_FLUSH_PENDING
    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
    .is_ok()
  {
    let _ = proxy.send_event(UserEvent::LiveUiFlush);
  }
}

fn queue_motor_bar(device_ip: String, value: f32, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_MOTOR_BARS.lock().unwrap().insert(device_ip, value);
  queue_live_ui_flush(proxy);
  let _ = proxy.send_event(UserEvent::ColliderProxFlush);
}

fn queue_pat_bar(param: String, graph: String, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_PAT_BARS.lock().unwrap().insert(param, graph);
  queue_live_ui_flush(proxy);
}

fn queue_prox_signal(device_ip: String, param: String, value: f32, proxy: &EventLoopProxy<UserEvent>) {
  let key = collider_viz::batch_key(&device_ip, &param);
  PENDING_PROX_SIGNALS.lock().unwrap().insert(key, value);
  let _ = proxy.send_event(UserEvent::ColliderProxFlush);
}

fn queue_headpat_telemetry(
  device_ip: String,
  param: String,
  json: String,
  proxy: &EventLoopProxy<UserEvent>,
) {
  let key = collider_viz::batch_key(&device_ip, &param);
  PENDING_HEADPAT_TELEMETRY
    .lock()
    .unwrap()
    .insert(key.clone(), json.clone());
  LAST_HEADPAT_TELEMETRY.lock().unwrap().insert(key, json);
  let _ = proxy.send_event(UserEvent::ColliderProxFlush);
}

/// Prefer live motor bar (post-`send_data`) over velocity-derived estimate in headpat telemetry JSON.
fn merge_headpat_telemetry_motor(json: &str, motor: f32) -> String {
  let mut value: serde_json::Value = serde_json::from_str(json).unwrap_or(serde_json::json!({}));
  if let Some(obj) = value.as_object_mut() {
    obj.insert("motor".to_string(), serde_json::json!(motor.clamp(0.0, 1.0)));
  }
  value.to_string()
}

fn flush_live_ui(webview: &wry::WebView) {
  loop {
    LIVE_UI_FLUSH_PENDING.store(false, Ordering::Release);
    let motor_batch: HashMap<String, f32> = PENDING_MOTOR_BARS.lock().unwrap().drain().collect();
    let pat_batch: HashMap<String, String> = PENDING_PAT_BARS.lock().unwrap().drain().collect();
    if motor_batch.is_empty() && pat_batch.is_empty() {
      break;
    }
    if !motor_batch.is_empty() {
      if let Ok(json) = serde_json::to_string(&motor_batch) {
        let _ = webview.evaluate_script(&format!("applyMotorBars({});", json));
      }
    }
    if !pat_batch.is_empty() {
      if let Ok(json) = serde_json::to_string(&pat_batch) {
        let _ = webview.evaluate_script(&format!("applyPatBars({});", json));
      }
    }
    if !LIVE_UI_FLUSH_PENDING.load(Ordering::Acquire) {
      break;
    }
  }
}

const OUTPUT_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body { height: 100%; min-height: 100vh; overflow-x: hidden; background: #000; }
::-webkit-scrollbar { width: 20px; height: 0; }
::-webkit-scrollbar-track { background: #000; }
::-webkit-scrollbar-thumb { background: #3f3f4e; border-radius: 10px; border: 2px solid #000; }
::-webkit-scrollbar-thumb:hover { background: #5b5b70; }
::-webkit-scrollbar-corner { background: #000; }
body {
  background: #000;
  color: #e8e8f0;
  font-family: "Segoe UI", system-ui, sans-serif;
  display: flex;
  flex-direction: column;
  overflow: hidden;
}
header {
  flex-shrink: 0;
  display: flex;
  justify-content: center;
  background: #000;
}
.header-inner {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  align-items: center;
  width: 100%;
  max-width: 1080px;
  min-height: 88px;
}
body.ui-large .header-inner { max-width: 2160px; }
body:has(#main.devices-centered-layout) .header-inner {
  grid-template-columns: minmax(0, 1fr) auto minmax(0, 1fr);
}
body:has(#main.devices-centered-layout) .header-col-config {
  grid-column: 2;
}
body:has(#main.devices-centered-layout) .header-col-log {
  grid-column: 3;
}
.header-col-config {
  display: flex;
  justify-content: center;
  align-items: center;
  min-width: 0;
}
.header-logo-col {
  display: flex;
  justify-content: center;
  align-items: center;
}
.header-col-log {
  display: flex;
  justify-content: flex-end;
  align-items: center;
  min-width: 0;
  padding: 0 32px 0 16px;
  box-sizing: border-box;
}
.header-right .btn {
  padding: 12px 18px;
  font-size: 1.35rem;
}
.header-logo {
  display: block;
  height: 88px;
  width: auto;
  max-width: 100%;
  object-fit: contain;
  object-position: center;
}
#main-center {
  flex: 1 1 0;
  min-height: 0;
  display: flex;
  justify-content: center;
  align-items: stretch;
  overflow: hidden;
  background: #000;
}
#main {
  display: grid;
  grid-template-columns: minmax(0, 1fr) minmax(0, 1fr);
  width: 100%;
  max-width: 1080px;
  min-height: 100%;
  height: 100%;
}
body.ui-large #main { max-width: 2160px; }
/* Console hidden: drop log column and center devices in the window. */
#main.devices-centered-layout {
  grid-template-columns: minmax(0, 1fr);
  justify-items: center;
}
#main.devices-centered-layout #log-section {
  display: none;
}
#main.devices-centered-layout #config-wrap {
  width: 100%;
  max-width: 1080px;
}
body.ui-large #main.devices-centered-layout #config-wrap {
  max-width: 2160px;
}
#main.devices-centered-layout #config-scroll {
  direction: rtl;
  display: flex;
  flex-direction: column;
  align-items: center;
}
#main.devices-centered-layout #device-list {
  width: 100%;
  max-width: 960px;
  padding-left: 32px;
  padding-right: 32px;
  box-sizing: border-box;
}
#main.devices-centered-layout .device-card {
  width: 100%;
  max-width: 960px;
}
#main.devices-centered-layout #config-wrap .config-footer {
  padding-left: 32px;
  padding-right: 32px;
  max-width: 960px;
  width: 100%;
  box-sizing: border-box;
  margin: 0 auto;
}
#main.devices-centered-layout #config-wrap .footer-toolbar {
  justify-content: center;
}
#main.devices-centered-layout #config-status {
  margin-left: auto;
  margin-right: auto;
  max-width: 960px;
}
#config-wrap {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  /* This column is zoomed (0.5) in normal mode. Use doubled spacing here so the
     *visual* inset matches the unzoomed log column. */
  padding: 32px 32px 0 32px;
  gap: 24px;
  overflow: hidden;
  position: relative;
  zoom: 0.5;
  transform-origin: top left;
}
body.ui-large #config-wrap {
  zoom: 1;
  padding: 16px 16px 0 16px;
  gap: 12px;
}
#config-column-divider {
  position: absolute;
  right: 0;
  width: 1px;
  background: #2a2a36;
  pointer-events: none;
  display: none !important;
}
#config-scroll {
  flex: 1 1 0;
  min-height: 0;
  overflow-x: auto;
  overflow-y: auto;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  direction: ltr;
}
#device-list,
#device-list .device-card,
.device-list-actions {
  direction: ltr;
}
#config-wrap .config-footer,
#config-wrap > .hint {
  flex-shrink: 0;
}
#config-wrap .config-footer {
  flex-shrink: 0;
  padding: 16px 6px 28px 70px;
  box-sizing: border-box;
  /* Stay the same visual size when VR MODE sets #config-wrap zoom to 1. */
  zoom: 1;
  transform-origin: bottom left;
}
body.ui-large #config-wrap .config-footer {
  zoom: 0.5;
}
#main.devices-centered-layout #config-wrap .config-footer {
  transform-origin: bottom center;
}
#config-wrap .footer-toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-start;
  width: 100%;
}
#config-wrap .footer-group {
  display: inline-flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}
#config-wrap .footer-group-settings {
  gap: 12px;
}
#config-wrap .footer-group-settings .btn {
  min-height: 2.85rem;
  white-space: nowrap;
}
#config-wrap .footer-group-settings .osc-port-row {
  gap: 12px;
}
#log-section {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 16px 16px 0 16px;
  gap: 12px;
  background: #000;
  overflow: hidden;
}
/* Log column visible when console and/or any visualizer is open. */
#log-section:not(.log-column-open) #log-cards-scroll,
#log-section:not(.log-column-open) #log-bottom-spacer {
  display: none !important;
}
#log-section:not(.console-expanded) .log-console-card {
  display: none !important;
}
#log-cards-scroll {
  flex: 1 1 0;
  min-width: 0;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  overscroll-behavior: contain;
  -webkit-overflow-scrolling: touch;
  padding-right: 16px;
  box-sizing: border-box;
}
#log-cards-list {
  display: flex;
  flex-direction: column;
  gap: 20px;
  width: 100%;
}
#log-viz-cards {
  display: flex;
  flex-direction: column;
  gap: 20px;
  width: 100%;
}
.log-card {
  width: 100%;
  background: #16161e;
  border: 1px solid #2a2a36;
  border-radius: 10px;
  overflow: hidden;
  flex-shrink: 0;
}
.log-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 10px 12px;
  border-bottom: 1px solid #2a2a36;
  background: #12121a;
}
.log-card-header h3 {
  margin: 0;
  font-size: 0.95rem;
  font-weight: 600;
  color: #c4b5fd;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.log-card-close {
  flex-shrink: 0;
  width: 1.75rem;
  height: 1.75rem;
  padding: 0;
  border-radius: 50%;
  border: 1px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
  font-size: 1.1rem;
  line-height: 1;
  cursor: pointer;
  font-family: inherit;
}
.log-card-close:hover {
  background: #3f3f4e;
}
.log-card-body {
  min-height: 0;
}
.log-viz-card .log-card-body {
  min-height: 480px;
  max-height: 720px;
  overflow-x: hidden;
  overflow-y: auto;
}
body.ui-large .log-viz-card {
  overflow: visible;
}
body.ui-large .log-viz-card .log-card-body {
  min-height: 0;
  max-height: none;
  overflow-x: hidden;
  overflow-y: visible;
}
.log-console-card .log-card-body {
  min-height: 10rem;
  max-height: 14rem;
  display: flex;
  flex-direction: column;
}
#log-box {
  display: flex;
  flex-direction: column;
  flex: 1 1 0;
  min-height: 0;
  width: 100%;
  padding: 10px 12px;
  box-sizing: border-box;
  overflow: hidden;
}
#log-bottom-spacer {
  flex-shrink: 0;
  width: 100%;
  box-sizing: border-box;
  padding-bottom: 16px;
}
{{COLLIDER_VIZ_STYLES}}
#pat-bars {
  flex-shrink: 0;
  font-family: "Cascadia Code", "Consolas", monospace;
  font-size: 13px;
  line-height: 1.5;
  color: #e879f9;
  min-height: 1.5em;
  white-space: pre;
  overflow: hidden;
  margin-bottom: 8px;
}
#pat-bars:empty {
  display: none;
  margin-bottom: 0;
}
#log {
  display: block;
  flex: 1 1 0;
  min-height: 0;
  box-sizing: border-box;
  width: 100%;
  max-width: 100%;
  margin: 0;
  overflow: hidden;
  font-family: "Cascadia Code", "Consolas", monospace;
  font-size: 12px;
  line-height: 1.45;
  color: #b8b8c8;
  padding: 0;
  background: transparent;
  border: none;
  border-radius: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
  word-break: break-word;
}
#config-status {
  flex-shrink: 0;
  margin-left: 32px;
  margin-right: 6px;
  font-size: 1.7rem;
  padding: 16px 24px;
  border-radius: 16px;
  display: none;
}
#config-status.err { display: block; background: #450a0a; color: #fca5a5; }
#device-list {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding-left: 32px;
  padding-right: 6px;
}
.device-list-actions {
  display: flex;
  align-items: center;
  gap: 16px;
  flex-wrap: wrap;
  padding: 24px 6px 8px 32px;
  box-sizing: border-box;
  width: 100%;
}
.device-list-actions .btn {
  min-height: 3.25rem;
  white-space: nowrap;
}
#main.devices-centered-layout .device-list-actions {
  max-width: 960px;
  padding-left: 32px;
  padding-right: 32px;
  margin-left: auto;
  margin-right: auto;
}
.device-card {
  width: 100%;
  /* test-slider-col 144px − horizontal padding 40px */
  --test-slider-track-width: calc(144px - 40px);
  background: #16161e;
  border: 2px solid #2a2a36;
  border-radius: 20px;
  overflow: hidden;
}
.device-card.is-collapsed .device-card-layout {
  min-height: 0;
  align-items: stretch;
}
.device-card.is-collapsed .device-card-body {
  display: none;
}
.device-card.is-collapsed .test-slider-col {
  align-self: stretch;
  flex: 0 0 144px;
  background: #16161e;
  border-left: 2px solid #2a2a36;
  border-radius: 0 18px 18px 0;
  justify-content: flex-start;
}
.device-card.is-collapsed .test-slider-track {
  display: none;
}
.device-card.is-collapsed .motor-indicator-square {
  display: block;
  pointer-events: auto;
  cursor: pointer;
  touch-action: none;
  user-select: none;
}
.device-card.is-collapsed .device-actions {
  margin-top: 0;
}
.device-card.is-collapsed .device-actions-start > .btn-danger {
  display: none;
}
.device-card.is-collapsed .device-actions-end .device-viz-btn {
  display: none;
}
.device-card-layout {
  display: flex;
  flex-direction: row;
  align-items: stretch;
  min-height: 440px;
}
.device-main {
  flex: 1;
  min-width: 0;
  padding: 28px;
  display: flex;
  flex-direction: column;
  gap: 24px;
}
.device-card h3 { font-size: 3.6rem; color: #c4b5fd; }
.device-name-input {
  width: 100%;
  font-size: 3.8rem;
  font-weight: 600;
  font-family: inherit;
  color: #c4b5fd;
  background: transparent;
  border: none;
  border-bottom: 2px dashed #3f3f4e;
  padding: 8px 0 12px;
  margin: 0 0 8px;
}
.device-name-input:focus {
  outline: none;
  border-bottom-color: #a855f7;
  border-bottom-style: solid;
}
.device-name-input::placeholder { color: #6b6b80; font-weight: 500; }
.device-name-row {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: nowrap;
  /* Align chrome buttons with device-setup-panel show/hide (panel uses 32px horizontal padding). */
  padding-right: 32px;
  box-sizing: border-box;
}
.device-name-row .device-name-input {
  flex: 1;
  min-width: 16rem;
  margin-bottom: 0;
}
.device-name-row-chrome {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-shrink: 0;
  margin-left: auto;
}
button.device-status {
  margin: 0;
  appearance: none;
  -webkit-appearance: none;
}
.device-status {
  flex-shrink: 0;
  box-sizing: border-box;
  width: 3rem;
  height: 3rem;
  min-width: 3rem;
  padding: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  border-radius: 50%;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
  color: #8888a0;
  cursor: pointer;
}
.device-status-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  line-height: 0;
}
.device-status-svg {
  width: 1.25rem;
  height: 1.25rem;
  display: block;
}
.device-status.online .device-status-svg,
.device-status.offline .device-status-svg {
  width: 1.65rem;
  height: 1.65rem;
}
.device-status.checking .device-status-spinner {
  animation: device-status-spin 0.75s linear infinite;
}
@keyframes device-status-spin {
  to { transform: rotate(360deg); }
}
.device-status:hover {
  filter: brightness(1.12);
}
.device-status:active {
  filter: brightness(0.95);
}
.device-status.online {
  color: #86efac;
  border-color: #166534;
  background: #14532d;
}
.device-status.offline {
  color: #fca5a5;
  border-color: #7f1d1d;
  background: #450a0a;
}
.device-status.checking {
  color: #c4b5fd;
  border-color: #5b21b6;
  background: #2e1065;
}
.device-status.unknown {
  color: #8888a0;
}
.btn-sm {
  padding: 12px 24px;
  font-size: 1.6rem;
}
.device-card label { display: flex; flex-direction: column; gap: 8px; font-size: 1.6rem; color: #a1a1b5; }
.device-card input:not(.device-name-input) {
  padding: 16px 20px;
  border-radius: 12px;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
  color: #e8e8f0;
  font-size: 1.8rem;
}
.device-card input.device-name-input {
  font-size: 3.8rem;
  padding: 12px 0 16px;
  background: transparent;
  border: none;
  border-bottom: 2px dashed #3f3f4e;
  border-radius: 0;
}
.device-card input:not([type="range"]):focus { outline: none; border-color: #a855f7; }
.device-card input.device-name-input:focus {
  border-bottom-color: #a855f7;
  border-bottom-style: solid;
}
.slider-field {
  display: flex;
  flex-direction: column;
  gap: 20px;
  width: 100%;
  margin-top: 16px;
  padding: 28px 32px;
  border-radius: 20px;
  background: #12121a;
  border: 2px solid #2a2a36;
  box-sizing: border-box;
}
.slider-field-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  min-height: 56px;
  padding: 0 8px;
}
.panel-disclosure-header {
  display: grid !important;
  grid-template-columns: minmax(0, 1fr) auto 3rem;
  align-items: center;
  column-gap: 24px;
  padding: 0 !important;
  min-height: 56px;
}
.panel-disclosure-header .panel-title-row,
.panel-disclosure-header .device-setup-panel-title,
.panel-disclosure-header .slider-field-title,
.panel-disclosure-header .proximity-band-panel-title {
  grid-column: 1;
  min-width: 0;
}
.panel-disclosure-header .device-setup-header-actions,
.panel-disclosure-header .power-panel-header-actions,
.panel-disclosure-header .velocity-panel-header-actions,
.panel-disclosure-header .proximity-band-header-actions {
  display: contents;
}
.panel-disclosure-header .speed-value {
  grid-column: 2;
  justify-self: end;
}
.panel-disclosure-header .velocity-switch {
  grid-column: 2;
  justify-self: end;
}
.panel-disclosure-header .disclosure-toggle-btn {
  grid-column: 3;
  justify-self: end;
}
.slider-field-title {
  font-size: 2rem;
  font-weight: 600;
  color: #e8e8f0;
  letter-spacing: 0.01em;
  line-height: 1.25;
}
.slider-field .speed-value {
  font-size: 2.25rem;
  min-width: 104px;
}
.power-panel-header-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}
.power-panel-header-actions .speed-value {
  min-width: 0;
  text-align: right;
  color: #c4b5fd;
}
.power-panel-body.hidden {
  display: none;
}
.power-panel:has(.power-panel-body.hidden) .power-panel-header-actions .speed-value {
  display: none;
}
.speed-slider-row {
  display: flex;
  align-items: center;
  gap: 32px;
  width: 100%;
  padding: 16px 8px 20px;
  box-sizing: border-box;
}
.speed-slider-row input[type="range"] {
  flex: 1;
  min-width: 0;
  width: 100%;
  height: var(--test-slider-track-width);
  margin: 0;
  padding: 0;
  border: none;
  -webkit-appearance: none;
  appearance: none;
  background: transparent;
  cursor: pointer;
  touch-action: none;
}
.speed-slider-row input[type="range"]:focus,
.speed-slider-row input[type="range"]:focus-visible {
  outline: none;
  border: none;
  box-shadow: none;
}
.speed-slider-row input[type="range"]::-webkit-slider-runnable-track {
  height: 40px;
  border-radius: 20px;
  background: #2a2a36;
  border: 2px solid #3f3f4e;
}
.speed-slider-row input[type="range"]::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: var(--test-slider-track-width);
  height: var(--test-slider-track-width);
  margin-top: calc((40px - var(--test-slider-track-width)) / 2);
  border-radius: 50%;
  background: linear-gradient(135deg, #e879f9, #7c3aed);
  border: 6px solid #f3e8ff;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.45);
}
.speed-slider-row input[type="range"]::-moz-range-track {
  height: 40px;
  border-radius: 20px;
  background: #2a2a36;
  border: 2px solid #3f3f4e;
}
.speed-slider-row input[type="range"]::-moz-range-thumb {
  width: var(--test-slider-track-width);
  height: var(--test-slider-track-width);
  border-radius: 50%;
  background: linear-gradient(135deg, #e879f9, #7c3aed);
  border: 6px solid #f3e8ff;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.45);
}
.speed-value {
  flex-shrink: 0;
  min-width: 112px;
  font-size: 2.3rem;
  font-weight: 600;
  color: #c4b5fd;
  text-align: right;
}
.device-fields {
  display: flex;
  flex-direction: column;
  gap: 24px;
}
.ip-input-row {
  display: flex;
  align-items: center;
  gap: 16px;
}
.ip-input-row input {
  flex: 1;
  min-width: 0;
}
.ip-input-row .btn-sm {
  flex-shrink: 0;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 16px 20px;
  border-radius: 12px;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
  color: #e8e8f0;
  font-size: 1.8rem;
  font-weight: 600;
  line-height: 1.2;
  box-sizing: border-box;
}
.ip-input-row .btn-sm:hover {
  background: #1a1a24;
  border-color: #5b5b6e;
}
.ip-input-row .btn-sm:disabled {
  opacity: 0.65;
  cursor: wait;
}
@keyframes mdns-found-pulse {
  0%, 100% {
    box-shadow: 0 0 0 0 rgba(168, 85, 247, 0.55);
    background: #6d28d9;
    border-color: #a855f7;
    color: #f3e8ff;
  }
  50% {
    box-shadow: 0 0 0 10px rgba(168, 85, 247, 0);
    background: #7c3aed;
    border-color: #c084fc;
    color: #faf5ff;
  }
}
.mdns-check-btn.found-pulse {
  animation: mdns-found-pulse 0.75s ease-in-out 3;
}
.mdns-check-hint {
  flex-shrink: 0;
  font-size: 1.45rem;
  font-weight: 600;
  color: #fca5a5;
  white-space: nowrap;
}
.velocity-panel {
  display: flex;
  flex-direction: column;
  gap: 24px;
  width: 100%;
  margin-top: 16px;
  padding: 28px 32px;
  border-radius: 20px;
  background: #12121a;
  border: 2px solid #2a2a36;
  box-sizing: border-box;
}
.velocity-panel-header {
  display: flex;
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
  margin: 0;
}
.panel-title-row {
  display: flex;
  align-items: center;
  gap: 16px;
  min-width: 0;
}
.velocity-panel-title {
  font-size: 2rem;
  font-weight: 600;
  color: #e8e8f0;
  letter-spacing: 0.01em;
}
.panel-info-btn {
  width: 36px;
  height: 36px;
  flex-shrink: 0;
  border-radius: 50%;
  border: 2px solid #6b6b80;
  background: #1a1a24;
  color: #b8b8c8;
  font-size: 1.36rem;
  font-weight: 700;
  font-family: Georgia, 'Times New Roman', serif;
  font-style: italic;
  line-height: 1;
  padding: 0;
  cursor: pointer;
}
.panel-info-btn:hover,
.panel-info-btn[aria-expanded="true"] {
  border-color: #a855f7;
  color: #f3e8ff;
  background: #2a2a36;
}
.panel-info-btn:focus {
  outline: none;
}
.panel-info-btn:focus-visible {
  box-shadow: 0 0 0 4px #000, 0 0 0 6px #a855f7;
}
.panel-info-text {
  margin: -8px 0 0;
  padding: 16px 20px;
  font-size: 1.5rem;
  line-height: 1.45;
  color: #a1a1b5;
  background: #0f0f14;
  border: 2px solid #2a2a36;
  border-radius: 12px;
}
.panel-info-text.hidden {
  display: none;
}
.velocity-panel-header-actions {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 12px;
}
.velocity-panel:not(.velocity-enabled) .headpat-panel-toggle {
  display: none;
}
.velocity-panel:not(.velocity-enabled) .velocity-switch {
  grid-column: 3;
  justify-self: end;
}
.velocity-panel-header .velocity-switch {
  cursor: pointer;
}
.velocity-panel-body {
  display: flex;
  flex-direction: column;
  gap: 24px;
  padding-top: 24px;
  border-top: 2px solid #2a2a36;
}
.velocity-panel-body.hidden {
  display: none;
}
.velocity-panel .slider-field {
  margin-top: 0;
  padding: 0;
  border: none;
  border-radius: 0;
  background: transparent;
  gap: 16px;
}
.velocity-panel .speed-slider-row {
  padding: 8px 0 12px;
}
.proximity-band-panel {
  display: flex;
  flex-direction: column;
  gap: 24px;
  width: 100%;
  margin-top: 16px;
  padding: 28px 32px;
  border-radius: 20px;
  background: #12121a;
  border: 2px solid #2a2a36;
  box-sizing: border-box;
}
.proximity-band-panel.hidden {
  display: none;
}
.proximity-band-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
}
.proximity-band-panel-title {
  font-size: 2rem;
  font-weight: 600;
  color: #e8e8f0;
  letter-spacing: 0.01em;
}
.proximity-band-header-actions {
  flex-shrink: 0;
  display: flex;
  gap: 12px;
  align-items: center;
}
.proximity-band-header-actions .btn-sm {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 20px;
  border-radius: 999px;
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
  font-size: 1.6rem;
  font-weight: 600;
  line-height: 1;
}
.proximity-band-header-actions .btn-sm:hover {
  background: #3f3f4e;
}
.proximity-band-panel-body {
  display: flex;
  flex-direction: column;
  gap: 24px;
}
.proximity-band-panel-body.hidden {
  display: none;
}
.proximity-band-panel .slider-field {
  margin-top: 0;
  padding: 0;
  border: none;
  border-radius: 0;
  background: transparent;
  gap: 16px;
}
.proximity-band-panel .speed-slider-row {
  padding: 8px 0 12px;
}
.device-setup-panel {
  display: flex;
  flex-direction: column;
  gap: 24px;
  width: 100%;
  margin-top: 0;
  margin-bottom: 8px;
  padding: 28px 32px;
  border-radius: 20px;
  background: #12121a;
  border: 2px solid #2a2a36;
  box-sizing: border-box;
}
.device-setup-panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 24px;
}
.device-setup-panel-title {
  font-size: 2rem;
  font-weight: 600;
  color: #e8e8f0;
  letter-spacing: 0.01em;
}
.device-setup-header-actions {
  flex-shrink: 0;
  display: flex;
  gap: 12px;
  align-items: center;
}
.device-setup-header-actions .btn-sm {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 20px;
  border-radius: 999px;
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
  font-size: 1.6rem;
  font-weight: 600;
  line-height: 1;
}
.device-setup-header-actions .btn-sm:hover {
  background: #3f3f4e;
}
.device-setup-panel-body {
  display: flex;
  flex-direction: column;
  gap: 24px;
}
.device-setup-panel-body.hidden {
  display: none;
}
.device-setup-panel .device-fields {
  display: flex;
  flex-direction: column;
  gap: 24px;
}
.velocity-toggle-row {
  position: relative;
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
  gap: 24px !important;
  margin: 0;
  cursor: pointer;
  user-select: none;
}
.velocity-toggle-label {
  font-size: 1.8rem;
  color: #d8d8e8;
  font-weight: 600;
}
.velocity-switch {
  position: relative;
  display: inline-flex;
  align-items: center;
  flex-shrink: 0;
  cursor: pointer;
}
.velocity-toggle-input {
  position: absolute;
  width: 1px;
  height: 1px;
  padding: 0;
  margin: -1px;
  overflow: hidden;
  clip: rect(0, 0, 0, 0);
  clip-path: inset(50%);
  white-space: nowrap;
  border: 0;
}
.velocity-toggle-track {
  display: block;
  position: relative;
  width: 88px;
  height: 48px;
  flex-shrink: 0;
  box-sizing: border-box;
  border: 3px solid #4a4a5c;
  border-radius: 999px;
  background: #2a2a36;
  transition: background 0.15s ease, border-color 0.15s ease, box-shadow 0.15s ease;
}
.velocity-toggle-thumb {
  position: absolute;
  top: 50%;
  left: 6px;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: #b8b8c8;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.35);
  transform: translateY(-50%);
  transition: transform 0.15s ease, background 0.15s ease;
  pointer-events: none;
}
.velocity-toggle-input:checked + .velocity-toggle-track {
  border-color: #e879f9;
  background: linear-gradient(135deg, #e879f9, #7c3aed);
}
.velocity-toggle-input:checked + .velocity-toggle-track .velocity-toggle-thumb {
  transform: translate(40px, -50%);
  background: #f3e8ff;
}
.velocity-toggle-input:focus { outline: none; }
.velocity-toggle-input:focus-visible + .velocity-toggle-track {
  box-shadow: 0 0 0 4px #12121a, 0 0 0 8px #a855f7;
}
.velocity-toggle-row.velocity-sub-toggle .velocity-toggle-label {
  font-size: 1.7rem;
  color: #a1a1b5;
  font-weight: 500;
}
.device-card label .hint {
  display: block;
  margin-top: 4px;
  line-height: 1.35;
}
.test-slider-col {
  flex: 0 0 144px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 16px;
  padding: 24px 20px;
  border-left: 2px solid #2a2a36;
  background: #0f0f14;
  align-self: stretch;
}
.test-slider-label {
  flex-shrink: 0;
  font-size: 1.5rem;
  color: #a1a1b5;
  font-weight: 600;
}
.test-slider-track {
  position: relative;
  flex: 1;
  width: var(--test-slider-track-width);
  max-width: 100%;
  min-height: 0;
  background: #0f0f14;
  border: 4px solid #3f3f4e;
  border-radius: 20px;
  overflow: hidden;
  cursor: pointer;
  touch-action: none;
  user-select: none;
}
.test-slider-track.active {
  border-color: #c026d3;
  box-shadow: 0 0 24px rgba(192, 38, 211, 0.35);
}
.test-slider-arrow {
  position: absolute;
  bottom: 16px;
  left: 50%;
  z-index: 2;
  width: 28px;
  height: 28px;
  margin-left: -14px;
  pointer-events: none;
  border-top: 5px solid #c4b5fd;
  border-right: 5px solid #c4b5fd;
  transform: rotate(-45deg);
  opacity: 0.85;
  filter: drop-shadow(0 2px 4px rgba(0, 0, 0, 0.5));
  transition: opacity 0.08s ease-out;
}
.test-slider-track.active .test-slider-arrow:not(.hidden) {
  border-color: #e879f9;
  opacity: 1;
}
.test-slider-arrow.hidden {
  opacity: 0;
  visibility: hidden;
}
.test-slider-fill {
  position: absolute;
  bottom: 0;
  left: 0;
  right: 0;
  height: 0%;
  z-index: 1;
  background: linear-gradient(to top, #7c3aed, #e879f9);
  pointer-events: none;
}
.motor-indicator-square {
  display: none;
  position: relative;
  flex-shrink: 0;
  width: var(--test-slider-track-width);
  height: var(--test-slider-track-width);
  --motor-level: 0;
  border: 4px solid #3f3f4e;
  border-radius: 20px;
  background: #0f0f14;
  box-sizing: border-box;
  overflow: hidden;
  pointer-events: none;
}
.motor-indicator-square::before {
  content: '';
  position: absolute;
  inset: 0;
  background: #a855f7;
  opacity: var(--motor-level);
  transition: opacity 0.08s ease-out;
}
.device-actions {
  display: flex;
  flex-wrap: nowrap;
  align-items: center;
  justify-content: flex-start;
  gap: 12px;
  margin-top: 32px;
  width: 100%;
}
.device-actions-start {
  display: flex;
  gap: 12px;
  flex-wrap: nowrap;
  align-items: center;
  flex-shrink: 0;
}
.device-actions-end {
  display: flex;
  gap: 12px;
  flex-wrap: nowrap;
  align-items: center;
  flex-shrink: 0;
  margin-left: auto;
}
.device-actions .btn[disabled] { opacity: 0.55; cursor: default; }
.device-actions button {
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 10.5rem;
  height: 3rem;
  padding: 0 20px;
  border-radius: 999px;
  font-size: 1.6rem;
  font-weight: 600;
  line-height: 1;
  font-family: inherit;
  cursor: pointer;
}
.device-actions .btn-secondary.btn-sm {
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
}
.device-actions .btn-secondary.btn-sm:hover {
  background: #3f3f4e;
}
.device-actions .btn-secondary.btn-sm.device-viz-btn {
  border: 2px solid #7c3aed;
  background: #2a2a36;
  color: #e8e8f0;
}
.device-actions .btn-secondary.btn-sm.device-viz-btn:hover {
  background: #322847;
  border-color: #a78bfa;
  color: #f3e8ff;
}
.device-actions .btn-secondary.btn-sm.device-viz-btn.device-viz-btn-active {
  border-color: #a855f7;
  background: #2e1065;
  color: #f3e8ff;
}
.device-actions .btn-primary.btn-sm {
  border: 2px solid #7f1d1d;
  background: #450a0a;
  color: #fca5a5;
}
.device-actions .btn-primary.btn-sm:hover {
  background: #7f1d1d;
}
.device-actions .btn-danger {
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #b8b8c8;
}
.device-actions .btn-danger:hover {
  background: #3f3f4e;
}
.device-actions .device-remove-btn {
  width: 3rem;
  height: 3rem;
  min-width: 3rem;
  max-width: 3rem;
  padding: 0;
  border-radius: 50%;
}
.device-actions .device-remove-btn:hover {
  border-color: #7f1d1d;
  background: #450a0a;
  color: #fca5a5;
}
.device-remove-icon {
  display: flex;
  align-items: center;
  justify-content: center;
  line-height: 0;
}
.device-remove-svg {
  width: 1.55rem;
  height: 1.55rem;
  display: block;
}
.device-name-row-chrome .disclosure-toggle-btn,
.device-actions .disclosure-toggle-btn,
.device-setup-header-actions .disclosure-toggle-btn,
.power-panel-header-actions .disclosure-toggle-btn,
.velocity-panel-header-actions .disclosure-toggle-btn,
.proximity-band-header-actions .disclosure-toggle-btn {
  box-sizing: border-box;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 3rem;
  height: 3rem;
  min-width: 3rem;
  max-width: 3rem;
  padding: 0;
  border-radius: 50%;
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
  flex-shrink: 0;
  cursor: pointer;
}
.device-name-row-chrome .disclosure-toggle-btn:hover,
.device-actions .disclosure-toggle-btn:hover,
.device-setup-header-actions .disclosure-toggle-btn:hover,
.power-panel-header-actions .disclosure-toggle-btn:hover,
.velocity-panel-header-actions .disclosure-toggle-btn:hover,
.proximity-band-header-actions .disclosure-toggle-btn:hover {
  background: #3f3f4e;
}
#console-panel-toggle.console-visible {
  border: 2px solid #a855f7;
}
.disclosure-toggle-icon {
  display: inline-block;
  font-size: 1.4rem;
  line-height: 1;
  transition: transform 0.15s ease;
}
.disclosure-toggle-btn[aria-expanded="true"] .disclosure-toggle-icon {
  transform: rotate(180deg);
}
.btn {
  padding: 18px 28px;
  font-size: 1.7rem;
  font-weight: 600;
  font-family: inherit;
  border: none;
  border-radius: 16px;
  cursor: pointer;
}
.btn-primary { background: #7c3aed; color: #fff; }
.btn-primary:hover { background: #6d28d9; }
.btn-secondary { background: #2a2a36; color: #e8e8f0; }
.btn-secondary:hover { background: #3f3f4e; }
.osc-port-row {
  display: inline-flex;
  align-items: center;
  gap: 16px;
}
#osc-mode-btn.osc-query-active {
  border: 2px solid #7c3aed;
  background: #2e1065;
  color: #e9d5ff;
}
#osc-mode-btn.osc-query-active:hover { background: #4c1d95; }
#ui-scale-btn.vr-mode-active {
  box-shadow: 0 0 0 2px #000, 0 0 0 4px #a855f7;
}
#ui-scale-btn.vr-mode-active:hover {
  box-shadow: 0 0 0 2px #000, 0 0 0 4px #c084fc;
}
.osc-port-input {
  display: none;
  width: calc(5ch + 52px);
  min-width: calc(5ch + 52px);
  flex-shrink: 0;
  padding: 18px 24px;
  font-size: 1.7rem;
  font-weight: 600;
  font-variant-numeric: tabular-nums;
  font-family: inherit;
  color: #e8e8f0;
  border-radius: 16px;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
  box-sizing: border-box;
}
.osc-port-input:focus {
  outline: none;
  border-color: #a855f7;
}
.btn-danger { background: #450a0a; color: #fca5a5; }
.btn-danger:hover { background: #7f1d1d; }
.hint { font-size: 1.6rem; color: #6b6b80; margin-top: 8px; }

#config-wrap #autostart-btn {
  border: 2px solid #3f3f4e;
}
#config-wrap #autostart-btn.autostart-on { border-color: #a855f7; }
#config-wrap #autostart-btn:disabled { opacity: 0.75; cursor: default; }
</style>
</head>
<body>
<header>
  <div class="header-inner">
    <div class="header-col-config">
      <div class="header-logo-col">
        <img class="header-logo" src="{{LOGO_URI}}" alt="GiggleTech">
      </div>
    </div>
    <div class="header-col-log">
      <div class="header-right">
        <button type="button" class="btn btn-secondary" id="ui-scale-btn" aria-pressed="false"
          onclick="toggleUiScale()">VR MODE</button>
      </div>
    </div>
  </div>
</header>
<div id="main-center">
  <div id="main" class="devices-centered-layout">
    <div id="config-wrap">
      <div id="config-column-divider" aria-hidden="true"></div>
      <div id="config-status"></div>
      <div id="config-scroll">
        <div id="device-list"></div>
        <div class="device-list-actions">
          <button type="button" class="btn btn-secondary" onclick="addDevice()">+ Add Device</button>
          <button type="button" class="btn btn-primary footer-save" onclick="saveConfig()">Save</button>
        </div>
      </div>
      <footer class="config-footer">
        <div class="footer-toolbar">
          <div class="footer-group footer-group-settings">
            <div class="osc-port-row">
              <button type="button" class="btn btn-secondary" id="osc-mode-btn" onclick="toggleOscMode()">OSC: Query</button>
              <input type="text" id="osc-port-input" class="osc-port-input" inputmode="numeric"
                placeholder="9001" maxlength="5" title="UDP listen port"
                onblur="commitOscPortInput()" onkeydown="if (event.key === 'Enter') commitOscPortInput()">
            </div>
            <button type="button" class="btn btn-secondary" id="autostart-btn" aria-pressed="false"
              onclick="toggleAutoStart()">Start with Windows</button>
            <button type="button" class="btn btn-secondary" id="console-panel-toggle" aria-pressed="false"
              aria-controls="log-cards-scroll" aria-label="Show console"
              onclick="toggleConsolePanel()">Console</button>
          </div>
        </div>
      </footer>
    </div>
    <section id="log-section">
      <div id="log-cards-scroll">
        <div id="log-cards-list">
          <div id="log-viz-cards"></div>
          <div class="log-card log-console-card" id="log-console-card">
            <div class="log-card-header">
              <h3>Console</h3>
            </div>
            <div class="log-card-body">
              <div id="log-box">
                <pre id="pat-bars" aria-live="polite"></pre>
                <pre id="log"></pre>
              </div>
            </div>
          </div>
        </div>
      </div>
      <div id="log-bottom-spacer" aria-hidden="true"></div>
    </section>
  </div>
</div>
<script>{{COLLIDER_VIZ_RUNTIME}}</script>
<script>
let autoStartEnabled = false;
let autoStartBusy = false;

function setAutoStartUi(enabled) {
  autoStartEnabled = !!enabled;
  const btn = document.getElementById('autostart-btn');
  if (!btn) return;
  btn.setAttribute('aria-pressed', autoStartEnabled ? 'true' : 'false');
  btn.classList.toggle('autostart-on', autoStartEnabled);
  btn.textContent = 'Start with Windows';
}

function setAutoStartBusy(busy) {
  autoStartBusy = !!busy;
  const btn = document.getElementById('autostart-btn');
  if (!btn) return;
  btn.disabled = autoStartBusy;
  btn.textContent = 'Start with Windows';
}

function toggleAutoStart() {
  if (autoStartBusy) return;
  setAutoStartBusy(true);
  window.ipc.postMessage('autostart-set:' + (autoStartEnabled ? '0' : '1'));
}

window.onAutoStartState = function(payload) {
  payload = payload || {};
  if (payload.error) setConfigStatus(payload.error, true);
  setAutoStartUi(!!payload.enabled);
  setAutoStartBusy(false);
};

window.ipc.postMessage('autostart-get');

function setUiLarge(enabled) {
  document.body.classList.toggle('ui-large', !!enabled);
  try { localStorage.setItem('uiLarge', enabled ? '1' : '0'); } catch (_) {}
  const btn = document.getElementById('ui-scale-btn');
  if (btn) {
    btn.classList.toggle('vr-mode-active', !!enabled);
    btn.setAttribute('aria-pressed', enabled ? 'true' : 'false');
  }
  syncLogSectionLayout();
  requestAnimationFrame(() => {
    if (window.colliderVizApi && window.colliderVizApi.relayoutAll) {
      window.colliderVizApi.relayoutAll();
    }
  });
}

function toggleUiScale() {
  setUiLarge(!document.body.classList.contains('ui-large'));
}

function readConsoleExpandedPref() {
  try {
    const v = localStorage.getItem('consoleExpanded');
    if (v === '0') return false;
    if (v === '1') return true;
    const legacy = localStorage.getItem('consolePanelVisible');
    if (legacy === '0') return false;
    if (legacy === '1') return true;
  } catch (_) {}
  return false;
}

let consoleExpanded = readConsoleExpandedPref();

function persistConsoleExpanded(expanded) {
  try {
    localStorage.setItem('consoleExpanded', expanded ? '1' : '0');
  } catch (_) {}
}

function hasOpenColliderViz() {
  const list = document.getElementById('log-viz-cards');
  return !!(list && list.querySelector('.log-viz-card'));
}

function isLogColumnOpen() {
  return consoleExpanded || hasOpenColliderViz();
}

function applyLogColumnUi() {
  const section = document.getElementById('log-section');
  const main = document.getElementById('main');
  const logOpen = isLogColumnOpen();
  if (section) {
    section.classList.toggle('log-column-open', logOpen);
    section.classList.toggle('console-expanded', consoleExpanded);
  }
  if (main) main.classList.toggle('devices-centered-layout', !logOpen);
  const btn = document.getElementById('console-panel-toggle');
  if (btn) {
    btn.setAttribute('aria-expanded', consoleExpanded ? 'true' : 'false');
    btn.setAttribute('aria-pressed', consoleExpanded ? 'true' : 'false');
    btn.classList.toggle('console-visible', consoleExpanded);
    btn.textContent = 'Console';
    btn.setAttribute('aria-label', consoleExpanded ? 'Hide console' : 'Show console');
  }
  syncLogSectionLayout();
  if (consoleExpanded && typeof lastStatusLines !== 'undefined' && lastStatusLines.length) {
    requestAnimationFrame(() => renderStatus(lastStatusLines));
  }
  requestAnimationFrame(() => {
    if (window.colliderVizApi && window.colliderVizApi.relayoutAll) {
      window.colliderVizApi.relayoutAll();
    }
  });
}

function applyConsolePanelUi() {
  applyLogColumnUi();
}

function toggleConsolePanel() {
  consoleExpanded = !consoleExpanded;
  persistConsoleExpanded(consoleExpanded);
  applyConsolePanelUi();
}
window.toggleConsolePanel = toggleConsolePanel;

(() => {
  let enabled = false;
  try { enabled = localStorage.getItem('uiLarge') === '1'; } catch (_) {}
  setUiLarge(enabled);
})();

let editorDevices = [];
let editorSpeedDefaults = { min: 5, max: 25 };
let editorVelocityDefault = false;
let editorVelocityOnProxDropDefault = false;
let editorVelocityProxDefaults = { outer: 0, inner: 1, scalar: 20, softcap: 35, smoothing_ms: 80 };
let editorPortRx = 'OSCQuery';
let devicePingStatus = {};
let mdnsCheckInFlight = false;
let mdnsHintTimers = {};
let pingDebounceTimer = null;
const PING_POLL_MS = 5000;
let pingPollTimer = null;
let pingInFlight = false;
let pendingRemoveIndex = null;
let colliderAdjustmentVisibleByIndex = (() => {
  try {
    const v = localStorage.getItem('colliderAdjustmentVisibleByIndex');
    if (v) return JSON.parse(v);
  } catch (_) {}
  return {};
})();

let deviceSetupVisibleByIndex = (() => {
  try {
    const v = localStorage.getItem('deviceSetupVisibleByIndex');
    if (v) return JSON.parse(v);
  } catch (_) {}
  return {};
})();

let powerPanelVisibleByIndex = (() => {
  try {
    const v = localStorage.getItem('powerPanelVisibleByIndex');
    if (v) return JSON.parse(v);
  } catch (_) {}
  return {};
})();

function isDeviceSetupVisible(index) {
  const v = deviceSetupVisibleByIndex[index];
  if (v === undefined) return false;
  return !!v;
}

function setDeviceSetupVisible(index, visible) {
  deviceSetupVisibleByIndex[index] = visible;
  try {
    localStorage.setItem('deviceSetupVisibleByIndex', JSON.stringify(deviceSetupVisibleByIndex));
  } catch (_) {}
}

function isColliderAdjustmentVisible(index) {
  const v = colliderAdjustmentVisibleByIndex[index];
  if (v === undefined) return false;
  return !!v;
}

function setColliderAdjustmentVisible(index, visible) {
  colliderAdjustmentVisibleByIndex[index] = visible;
  try {
    localStorage.setItem('colliderAdjustmentVisibleByIndex', JSON.stringify(colliderAdjustmentVisibleByIndex));
  } catch (_) {}
}

function isPowerPanelVisible(index) {
  const v = powerPanelVisibleByIndex[index];
  if (v === undefined) return false;
  return !!v;
}

function setPowerPanelVisible(index, visible) {
  powerPanelVisibleByIndex[index] = visible;
  try {
    localStorage.setItem('powerPanelVisibleByIndex', JSON.stringify(powerPanelVisibleByIndex));
  } catch (_) {}
}

let headpatPanelVisibleByIndex = (() => {
  try {
    const v = localStorage.getItem('headpatPanelVisibleByIndex');
    if (v) return JSON.parse(v);
  } catch (_) {}
  return {};
})();

function isHeadpatPanelVisible(index) {
  const v = headpatPanelVisibleByIndex[index];
  if (v === undefined) return false;
  return !!v;
}

function setHeadpatPanelVisible(index, visible) {
  headpatPanelVisibleByIndex[index] = visible;
  try {
    localStorage.setItem('headpatPanelVisibleByIndex', JSON.stringify(headpatPanelVisibleByIndex));
  } catch (_) {}
}

function hasHeadpatPanelPreference(index) {
  return String(index) in headpatPanelVisibleByIndex;
}

let deviceCardCollapsedByIndex = (() => {
  try {
    const v = localStorage.getItem('deviceCardCollapsedByIndex');
    if (v) return JSON.parse(v);
  } catch (_) {}
  return {};
})();

function isDeviceCardCollapsed(index) {
  return !!deviceCardCollapsedByIndex[index];
}

function setDeviceCardCollapsed(index, collapsed) {
  deviceCardCollapsedByIndex[index] = collapsed;
  try {
    localStorage.setItem('deviceCardCollapsedByIndex', JSON.stringify(deviceCardCollapsedByIndex));
  } catch (_) {}
}

function applyDeviceCardCollapsedUi(index, collapsed) {
  const card = document.querySelector('.device-card[data-device-index="' + index + '"]');
  if (!card) return;
  card.classList.toggle('is-collapsed', collapsed);
  const btn = document.getElementById('device-card-toggle-' + index);
  if (btn) {
    btn.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
    btn.setAttribute('aria-label', (collapsed ? 'Show' : 'Hide') + ' device card details for device ' + (index + 1));
  }
  if (collapsed) {
    const d = editorDevices[index];
    const ipKey = d ? (d.ip || '').trim() : '';
    const level = ipKey && motorBarByIp[ipKey] != null ? motorBarByIp[ipKey] : 0;
    setMotorVisualForDevice(index, level);
  }
}
let activeTestDrag = null;
let testSliderSafetyReady = false;
const SPEED_SLIDER_STEPS = 1000;
const SPEED_SLIDER_LOW_MAX = 50;
const SPEED_SLIDER_SPLIT = 0.75;

/** Slider 0..1 → power % (min..100): first ¾ is min–50, last ¼ is 50–100. */
function sliderPosToSpeed(t) {
  t = Math.max(0, Math.min(1, t));
  const min = editorSpeedDefaults.min;
  let raw;
  if (t <= SPEED_SLIDER_SPLIT) {
    raw = SPEED_SLIDER_LOW_MAX * (t / SPEED_SLIDER_SPLIT);
  } else {
    const u = (t - SPEED_SLIDER_SPLIT) / (1 - SPEED_SLIDER_SPLIT);
    raw = SPEED_SLIDER_LOW_MAX + (100 - SPEED_SLIDER_LOW_MAX) * u;
  }
  return Math.round(min + (raw / 100) * (100 - min));
}

function speedToSliderPos(speed) {
  const min = editorSpeedDefaults.min;
  speed = Math.max(min, Math.min(100, speed));
  const raw = ((speed - min) / (100 - min)) * 100;
  if (raw <= SPEED_SLIDER_LOW_MAX) {
    return SPEED_SLIDER_SPLIT * (raw / SPEED_SLIDER_LOW_MAX);
  }
  return SPEED_SLIDER_SPLIT
    + (1 - SPEED_SLIDER_SPLIT) * ((raw - SPEED_SLIDER_LOW_MAX) / (100 - SPEED_SLIDER_LOW_MAX));
}

function setConfigStatus(msg, isError) {
  const el = document.getElementById('config-status');
  if (!el) return;
  if (!isError) {
    el.textContent = '';
    el.className = '';
    return;
  }
  el.textContent = msg;
  el.className = 'err';
}

function clearConfigStatus() {
  setConfigStatus('', false);
}

function isValidIp(ip) {
  const s = (ip || '').trim();
  if (!s) return false;
  const v4 = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/;
  const m = s.match(v4);
  if (m) return m.slice(1, 5).every((o) => Number(o) <= 255);
  if (s.includes(':')) return /^[0-9a-fA-F:.]+$/.test(s);
  return false;
}

function proxToSliderPct(p) {
  return Math.round(Math.max(0, Math.min(1, p)) * 100);
}
function sliderPctToProx(pct) {
  return Math.round(pct) / 100;
}
/** Collider sliders: left = close/high proximity, right = far/low (inverted range input). */
function proxToColliderSliderPct(p) {
  return 100 - proxToSliderPct(p);
}
function colliderSliderPctToProx(pct) {
  return sliderPctToProx(100 - pct);
}
function effectiveOuterProx(d) {
  return d.outer_proximity ?? editorVelocityProxDefaults.outer;
}
function effectiveInnerProx(d) {
  return d.inner_proximity ?? editorVelocityProxDefaults.inner;
}
function effectiveVelocityScalar(d) {
  return d.velocity_scalar ?? editorVelocityProxDefaults.scalar;
}
function effectiveVelocitySoftcap(d) {
  return d.velocity_softcap ?? editorVelocityProxDefaults.softcap;
}
/** Damping slider 0–100: left = none, right = max. Stored softcap is inverted (backend unchanged). */
function velocitySoftcapToDampingPct(softcap) {
  return Math.min(100, Math.max(0, 100 - softcap));
}
function dampingPctToVelocitySoftcap(dampingPct) {
  return Math.max(1, Math.min(100, 100 - dampingPct));
}
function effectiveVelocityDampingPct(d) {
  return velocitySoftcapToDampingPct(effectiveVelocitySoftcap(d));
}
function effectiveVelocitySmoothingMs(d) {
  return d.velocity_smoothing_ms ?? editorVelocityProxDefaults.smoothing_ms;
}

function deviceDisplayName(index, name) {
  const trimmed = (name || '').trim();
  if (trimmed) return trimmed;
  return index === 0 ? 'Headpats' : 'Device ' + (index + 1);
}

function colliderVizPayload(index) {
  const d = editorDevices[index];
  if (!d) return null;
  return {
    index: index,
    name: deviceDisplayName(index, d.name),
    device_ip: (d.ip || '').trim(),
    proximity_parameter: (d.proximity_parameter || 'proximity_01').trim(),
    outer: effectiveOuterProx(d),
    inner: effectiveInnerProx(d),
    velocity: !!d.use_velocity_control,
    velocity_scalar: effectiveVelocityScalar(d),
    velocity_softcap: effectiveVelocitySoftcap(d),
    velocity_smoothing_ms: effectiveVelocitySmoothingMs(d),
    velocity_on_prox_drop: !!d.velocity_on_prox_drop
  };
}

const COLLIDER_VIZ_CARD_INNER_HTML = {{COLLIDER_VIZ_CARD_INNER}};

function openColliderVizCard(payload) {
  const index = payload.index;
  const list = document.getElementById('log-viz-cards');
  if (!list) return;
  const name = (payload.name || '').trim() || ('Device ' + (index + 1));
  let card = list.querySelector('.log-viz-card[data-viz-index="' + index + '"]');
  if (!card) {
    card = document.createElement('div');
    card.className = 'log-card log-viz-card';
    card.dataset.vizIndex = String(index);
    card.innerHTML =
      '<div class="log-card-header">' +
        '<h3>' + escapeHtml(name) + '</h3>' +
        '<button type="button" class="log-card-close" aria-label="Close visualizer for device ' + (index + 1) + '" ' +
          'onclick="closeColliderViz(' + index + ')">&times;</button>' +
      '</div>' +
      '<div class="log-card-body">' + COLLIDER_VIZ_CARD_INNER_HTML + '</div>';
    list.appendChild(card);
    const root = card.querySelector('.collider-viz-root');
    if (root && window.colliderVizApi) window.colliderVizApi.mount(root, index);
  } else {
    const h3 = card.querySelector('.log-card-header h3');
    if (h3) h3.textContent = name;
  }
  if (window.colliderVizApi) window.colliderVizApi.applyState(payload);
  applyLogColumnUi();
}

function isColliderVizOpen(index) {
  const list = document.getElementById('log-viz-cards');
  return !!(list && list.querySelector('.log-viz-card[data-viz-index="' + index + '"]'));
}

function updateColliderVizButton(index) {
  const btn = document.querySelector('.device-viz-btn[data-viz-btn-index="' + index + '"]');
  if (!btn) return;
  const open = isColliderVizOpen(index);
  btn.classList.toggle('device-viz-btn-active', open);
  btn.setAttribute('aria-pressed', open ? 'true' : 'false');
  btn.setAttribute('aria-label', (open ? 'Close' : 'Open') + ' visualizer for device ' + (index + 1));
}

function closeColliderViz(index) {
  const list = document.getElementById('log-viz-cards');
  const card = list && list.querySelector('.log-viz-card[data-viz-index="' + index + '"]');
  if (card) {
    if (window.colliderVizApi) window.colliderVizApi.unmount(index);
    card.remove();
  }
  window.ipc.postMessage('collider-viz-close:' + index);
  updateColliderVizButton(index);
  applyLogColumnUi();
}

function syncColliderViz(index) {
  const p = colliderVizPayload(index);
  if (!p) return;
  const list = document.getElementById('log-viz-cards');
  if (list && list.querySelector('.log-viz-card[data-viz-index="' + index + '"]')) {
    if (window.colliderVizApi) window.colliderVizApi.applyState(p);
  }
  window.ipc.postMessage('collider-viz-update:' + JSON.stringify(p));
}

function openColliderViz(index) {
  if (isColliderVizOpen(index)) {
    closeColliderViz(index);
    return;
  }
  const p = colliderVizPayload(index);
  if (!p) return;
  openColliderVizCard(p);
  window.ipc.postMessage('collider-viz-open:' + JSON.stringify(p));
  updateColliderVizButton(index);
}

function editorValidationOk() {
  if (!editorDevices.length) return false;
  for (const d of editorDevices) {
    if (!d.ip.trim() || !isValidIp(d.ip)) return false;
    if (!(d.proximity_parameter || '').trim()) return false;
    if (d.max_speed < editorSpeedDefaults.min || d.max_speed > 100) return false;
    const outer = effectiveOuterProx(d);
    const inner = effectiveInnerProx(d);
    if (inner <= outer) return false;
    if (d.use_velocity_control) {
      const scalar = effectiveVelocityScalar(d);
      if (scalar < 1 || scalar > 100) return false;
      const softcap = effectiveVelocitySoftcap(d);
      if (softcap < 1 || softcap > 100) return false;
      const smoothing = effectiveVelocitySmoothingMs(d);
      if (smoothing < 0 || smoothing > 500) return false;
    }
  }
  return true;
}

function maybeClearConfigError() {
  const el = document.getElementById('config-status');
  if (!el || el.className !== 'err') return;
  const msg = el.textContent || '';
  if (msg.includes('Enter an IP')) {
    if (editorDevices.some((d) => isValidIp(d.ip))) clearConfigStatus();
    return;
  }
  if (editorValidationOk()) clearConfigStatus();
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

function renderDevices() {
  endActiveTestDrag();
  const list = document.getElementById('device-list');
  if (!editorDevices.length) {
    list.innerHTML = '<p class="hint">No devices yet. Click Add Device.</p>';
    return;
  }
  list.innerHTML = editorDevices.map((d, i) => `
    <div class="device-card${isDeviceCardCollapsed(i) ? ' is-collapsed' : ''}" data-device-index="${i}">
      <div class="device-card-layout">
        <div class="device-main">
          <div class="device-name-row">
            <input type="text" class="device-name-input" value="${escapeHtml(d.name || '')}"
              placeholder="${i === 0 ? 'Headpats' : 'Device ' + (i + 1)}"
              oninput="editorDevices[${i}].name=this.value; maybeClearConfigError()"
              aria-label="Device name">
            <div class="device-name-row-chrome">
              <button type="button" class="device-status unknown" id="device-status-${i}"
                aria-label="Ping device ${i + 1}: Unknown"
                onclick="pingDevice(${i}, true)">${pingStatusIconMarkup('unknown')}</button>
              <button type="button" class="btn btn-secondary btn-sm disclosure-toggle-btn" id="device-card-toggle-${i}"
                aria-expanded="${isDeviceCardCollapsed(i) ? 'false' : 'true'}"
                aria-label="${isDeviceCardCollapsed(i) ? 'Show' : 'Hide'} device card details for device ${i + 1}"
                onclick="toggleDeviceCardCollapse(${i})"><span class="disclosure-toggle-icon" aria-hidden="true">▼</span></button>
            </div>
          </div>
          <div class="device-card-body">
          <section class="device-setup-panel" aria-label="Device setup for device ${i + 1}">
            <div class="device-setup-panel-header panel-disclosure-header">
              <span class="device-setup-panel-title">Device setup</span>
              <div class="device-setup-header-actions">
                <button type="button" class="btn btn-secondary btn-sm disclosure-toggle-btn" id="device-setup-toggle-${i}"
                  aria-expanded="${isDeviceSetupVisible(i) ? 'true' : 'false'}"
                  aria-controls="device-setup-${i}"
                  aria-label="${isDeviceSetupVisible(i) ? 'Hide' : 'Show'} device setup for device ${i + 1}"
                  onclick="toggleDeviceSetup(${i})"><span class="disclosure-toggle-icon" aria-hidden="true">▼</span></button>
              </div>
            </div>
            <div class="device-setup-panel-body${isDeviceSetupVisible(i) ? '' : ' hidden'}" id="device-setup-${i}">
              <div class="device-fields">
                <label>IP address
                  <div class="ip-input-row">
                    <input type="text" id="device-ip-${i}" value="${escapeHtml(d.ip)}"
                      oninput="editorDevices[${i}].ip=this.value; onDeviceIpChange(${i}); maybeClearConfigError()">
                    <button type="button" class="btn btn-secondary btn-sm mdns-check-btn" id="mdns-check-btn-${i}" onclick="checkDeviceMdns(${i})">Search IP</button>
                    <span class="mdns-check-hint" id="mdns-check-hint-${i}"></span>
                  </div>
                </label>
                <label>Proximity parameter
                  <input type="text" value="${escapeHtml(d.proximity_parameter)}" placeholder="proximity_01"
                    oninput="editorDevices[${i}].proximity_parameter=this.value; maybeClearConfigError()">
                </label>
                <label>Max speed parameter
                  <input type="text" value="${escapeHtml(d.max_speed_parameter || '')}"
                    placeholder="(optional)"
                    oninput="editorDevices[${i}].max_speed_parameter=this.value; maybeClearConfigError()">
                </label>
              </div>
            </div>
          </section>
          <section class="slider-field power-panel" aria-label="Power for device ${i + 1}">
            <div class="slider-field-header panel-disclosure-header">
              <span class="slider-field-title">Power</span>
              <div class="power-panel-header-actions">
                <span class="speed-value" id="max-speed-val-${i}">${d.max_speed}%</span>
                <button type="button" class="btn btn-secondary btn-sm disclosure-toggle-btn" id="power-panel-toggle-${i}"
                  aria-expanded="${isPowerPanelVisible(i) ? 'true' : 'false'}"
                  aria-controls="power-panel-${i}"
                  aria-label="${isPowerPanelVisible(i) ? 'Hide' : 'Show'} power for device ${i + 1}"
                  onclick="togglePowerPanel(${i})"><span class="disclosure-toggle-icon" aria-hidden="true">▼</span></button>
              </div>
            </div>
            <div class="power-panel-body${isPowerPanelVisible(i) ? '' : ' hidden'}" id="power-panel-${i}">
              <div class="speed-slider-row">
                <input type="range" id="max-speed-slider-${i}" min="0" max="${SPEED_SLIDER_STEPS}"
                  aria-label="Power for device ${i + 1}"
                  value="${Math.round(speedToSliderPos(d.max_speed) * SPEED_SLIDER_STEPS)}"
                  oninput="onMaxSpeedChange(${i}, this)" onchange="saveConfig(true)">
              </div>
            </div>
          </section>
          <section class="velocity-panel${d.use_velocity_control ? ' velocity-enabled' : ''}" aria-label="Headpat Mode for device ${i + 1}">
            <div class="velocity-panel-header panel-disclosure-header">
              <div class="panel-title-row">
                <span class="velocity-panel-title">Headpat Mode</span>
                <button type="button" class="panel-info-btn" aria-expanded="false"
                  aria-controls="velocity-info-${i}"
                  aria-label="About Headpat Mode"
                  onclick="togglePanelInfo(event, 'velocity-info-${i}')">i</button>
              </div>
              <div class="velocity-panel-header-actions">
                <label class="velocity-switch">
                  <input type="checkbox" class="velocity-toggle-input" role="switch"
                    aria-label="Enable Headpat Mode for device ${i + 1}"
                    ${d.use_velocity_control ? 'checked' : ''}
                    onchange="onVelocityControlChange(${i}, this)">
                  <span class="velocity-toggle-track" aria-hidden="true">
                    <span class="velocity-toggle-thumb"></span>
                  </span>
                </label>
                <button type="button" class="btn btn-secondary btn-sm disclosure-toggle-btn headpat-panel-toggle" id="headpat-panel-toggle-${i}"
                  aria-expanded="${isHeadpatPanelVisible(i) ? 'true' : 'false'}"
                  aria-controls="headpat-panel-${i}"
                  aria-label="${isHeadpatPanelVisible(i) ? 'Hide' : 'Show'} headpat settings for device ${i + 1}"
                  onclick="toggleHeadpatPanel(${i})"><span class="disclosure-toggle-icon" aria-hidden="true">▼</span></button>
              </div>
            </div>
            <p class="panel-info-text hidden" id="velocity-info-${i}">
              Vibratrion strength follows how fast proximity changes, not how close you are.
            </p>
            <div class="velocity-panel-body${d.use_velocity_control && isHeadpatPanelVisible(i) ? '' : ' hidden'}" id="headpat-panel-${i}">
            <label class="velocity-toggle-row velocity-sub-toggle">
              <span class="velocity-toggle-label">Vibrate on pull-away</span>
              <input type="checkbox" class="velocity-toggle-input" role="switch"
                id="velocity-on-drop-${i}"
                aria-label="Vibrate on proximity drop for device ${i + 1}"
                ${d.velocity_on_prox_drop ? 'checked' : ''}
                onchange="onVelocityOnProxDropChange(${i}, this)">
              <span class="velocity-toggle-track" aria-hidden="true">
                <span class="velocity-toggle-thumb"></span>
              </span>
            </label>
            <label class="slider-field">
              <div class="slider-field-header">
                <span class="slider-field-title">Sensitivity</span>
                <span class="speed-value" id="velocity-scalar-val-${i}">${effectiveVelocityScalar(d)}%</span>
              </div>
              <div class="speed-slider-row">
                <input type="range" id="velocity-scalar-${i}" min="1" max="100"
                  value="${effectiveVelocityScalar(d)}"
                  aria-label="Velocity sensitivity for device ${i + 1}"
                  oninput="onVelocityScalarChange(${i}, this)" onchange="saveConfig(true)">
              </div>
            </label>
            <label class="slider-field">
              <div class="slider-field-header">
                <span class="slider-field-title">High-speed damping</span>
                <span class="speed-value" id="velocity-softcap-val-${i}">${effectiveVelocityDampingPct(d)}%</span>
              </div>
              <div class="speed-slider-row">
                <input type="range" id="velocity-softcap-${i}" min="0" max="100"
                  value="${effectiveVelocityDampingPct(d)}"
                  aria-label="Velocity high-speed damping for device ${i + 1}"
                  oninput="onVelocitySoftcapChange(${i}, this)" onchange="saveConfig(true)">
              </div>
            </label>
            <label class="slider-field">
              <div class="slider-field-header">
                <span class="slider-field-title">Smoothing</span>
                <span class="speed-value" id="velocity-smoothing-val-${i}">${effectiveVelocitySmoothingMs(d)}ms</span>
              </div>
              <div class="speed-slider-row">
                <input type="range" id="velocity-smoothing-${i}" min="0" max="250" step="5"
                  value="${effectiveVelocitySmoothingMs(d)}"
                  aria-label="Velocity smoothing for device ${i + 1}"
                  oninput="onVelocitySmoothingChange(${i}, this)" onchange="saveConfig(true)">
              </div>
            </label>
            </div>
          </section>
          <section class="proximity-band-panel" aria-label="Collider adjustment for device ${i + 1}">
            <div class="proximity-band-panel-header panel-disclosure-header">
              <span class="proximity-band-panel-title">Collider adjustment</span>
              <div class="proximity-band-header-actions">
                <button type="button" class="btn btn-secondary btn-sm disclosure-toggle-btn" id="collider-adjust-toggle-${i}"
                  aria-expanded="${isColliderAdjustmentVisible(i) ? 'true' : 'false'}"
                  aria-controls="proximity-band-${i}"
                  aria-label="${isColliderAdjustmentVisible(i) ? 'Hide' : 'Show'} collider adjustment for device ${i + 1}"
                  onclick="toggleColliderAdjustment(${i})"><span class="disclosure-toggle-icon" aria-hidden="true">▼</span></button>
              </div>
            </div>
            <div class="proximity-band-panel-body${isColliderAdjustmentVisible(i) ? '' : ' hidden'}" id="proximity-band-${i}">
              <label class="slider-field">
                <div class="slider-field-header">
                  <span class="slider-field-title">Inner</span>
                </div>
                <div class="speed-slider-row">
                  <input type="range" id="inner-prox-${i}" min="0" max="100"
                    value="${proxToColliderSliderPct(effectiveInnerProx(d))}"
                    aria-label="Inner proximity (close edge) for device ${i + 1}"
                    oninput="onInnerProxBandChange(${i}, this)" onchange="saveConfig(true)">
                </div>
              </label>
              <label class="slider-field">
                <div class="slider-field-header">
                  <span class="slider-field-title">Outer</span>
                </div>
                <div class="speed-slider-row">
                  <input type="range" id="outer-prox-${i}" min="0" max="100"
                    value="${proxToColliderSliderPct(effectiveOuterProx(d))}"
                    aria-label="Outer proximity (far edge) for device ${i + 1}"
                    oninput="onOuterProxBandChange(${i}, this)" onchange="saveConfig(true)">
                </div>
              </label>
            </div>
          </section>
          </div>
          <div class="device-actions">
            <div class="device-actions-start">
              ${pendingRemoveIndex === i
                ? `<button type="button" class="btn btn-danger" onclick="cancelRemoveDevice()">Remove</button>
                   <button type="button" class="btn btn-secondary btn-sm" onclick="cancelRemoveDevice()">Cancel</button>
                   <button type="button" class="btn btn-primary btn-sm" onclick="confirmRemoveDevice(${i})">Confirm</button>`
                : `<button type="button" class="btn btn-danger device-remove-btn" aria-label="Remove device ${i + 1}"
                   onclick="requestRemoveDevice(${i})">${removeDeviceIconMarkup()}</button>`}
            </div>
            <div class="device-actions-end">
              ${pendingRemoveIndex !== i ? `<button type="button" class="btn btn-sm btn-secondary device-viz-btn${isColliderVizOpen(i) ? ' device-viz-btn-active' : ''}"
                data-viz-btn-index="${i}" aria-pressed="${isColliderVizOpen(i) ? 'true' : 'false'}"
                aria-label="${isColliderVizOpen(i) ? 'Close' : 'Open'} visualizer for device ${i + 1}"
                onclick="openColliderViz(${i})">Visualizer</button>` : ''}
            </div>
          </div>
        </div>
        <div class="test-slider-col">
          <span class="test-slider-label">Motor</span>
          <div class="test-slider-track" data-index="${i}">
            <span class="test-slider-arrow" aria-hidden="true"></span>
            <div class="test-slider-fill"></div>
          </div>
          <div class="motor-indicator-square" data-index="${i}"
            aria-label="Motor test at full power for device ${i + 1}"></div>
        </div>
      </div>
    </div>
  `).join('');
  bindDeviceSliders();
  updatePingBadges();
  syncLogSectionLayout();
}

function syncLogSectionLayout() {
  const footer = document.querySelector('#config-wrap .config-footer');
  const spacer = document.getElementById('log-bottom-spacer');
  const logColumnOpen = isLogColumnOpen();
  if (spacer) {
    if (footer && logColumnOpen) {
      spacer.style.height = footer.offsetHeight + 'px';
    } else {
      spacer.style.height = '0px';
    }
  }

  const wrap = document.getElementById('config-wrap');
  const list = document.getElementById('device-list');
  if (!wrap || !list) return;
}

function pingStatusLabel(st) {
  if (st === 'online') return 'Online';
  if (st === 'offline') return 'Offline';
  if (st === 'checking') return 'Checking';
  return 'Unknown';
}

function removeDeviceIconMarkup() {
  return '<span class="device-remove-icon" aria-hidden="true"><svg class="device-remove-svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 7h16"/><path d="M10 7V5a2 2 0 0 1 4 0v2"/><path d="M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12"/><path d="M10 11v5"/><path d="M14 11v5"/></svg></span>';
}

function pingStatusIconMarkup(st) {
  let svg = '';
  if (st === 'online') {
    svg = '<svg class="device-status-svg" viewBox="0 0 24 24"><path d="M5 12.5l5.5 5.5L19 7.5" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"/></svg>';
  } else if (st === 'offline') {
    svg = '<svg class="device-status-svg" viewBox="0 0 24 24"><path d="M7 7l10 10M17 7L7 17" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round"/></svg>';
  } else if (st === 'checking') {
    svg = '<svg class="device-status-svg device-status-spinner" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2" opacity="0.28"/><path d="M12 4a8 8 0 0 1 8 8" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round"/></svg>';
  } else {
    svg = '<svg class="device-status-svg" viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2"/></svg>';
  }
  return '<span class="device-status-icon" aria-hidden="true">' + svg + '</span>';
}

function updatePingBadges() {
  editorDevices.forEach((d, i) => {
    const el = document.getElementById('device-status-' + i);
    if (!el) return;
    const ip = (d.ip || '').trim();
    const st = ip ? (devicePingStatus[ip] || 'unknown') : 'unknown';
    el.className = 'device-status ' + st;
    el.innerHTML = pingStatusIconMarkup(st);
    el.setAttribute('aria-label', 'Ping device ' + (i + 1) + ': ' + pingStatusLabel(st));
  });
}

function clearMdnsHint(index) {
  const hint = document.getElementById('mdns-check-hint-' + index);
  if (hint) hint.textContent = '';
  if (mdnsHintTimers[index]) {
    clearTimeout(mdnsHintTimers[index]);
    delete mdnsHintTimers[index];
  }
}

function showMdnsNotFound(index) {
  clearMdnsHint(index);
  const hint = document.getElementById('mdns-check-hint-' + index);
  if (!hint) return;
  hint.textContent = 'Not found';
  mdnsHintTimers[index] = setTimeout(() => {
    if (hint.textContent === 'Not found') hint.textContent = '';
    delete mdnsHintTimers[index];
  }, 3000);
}

function pulseMdnsCheckButton(index) {
  const btn = document.getElementById('mdns-check-btn-' + index);
  if (!btn) return;
  btn.classList.remove('found-pulse');
  void btn.offsetWidth;
  btn.classList.add('found-pulse');
  setTimeout(() => btn.classList.remove('found-pulse'), 2400);
}

function checkDeviceMdns(index) {
  if (mdnsCheckInFlight) return;
  mdnsCheckInFlight = true;
  clearMdnsHint(index);
  const btn = document.getElementById('mdns-check-btn-' + index);
  if (btn) btn.disabled = true;
  window.ipc.postMessage('mdns-check:' + JSON.stringify({ device_index: index }));
}

window.onDeviceMdnsResult = function(result) {
  mdnsCheckInFlight = false;
  const i = result.device_index;
  const btn = document.getElementById('mdns-check-btn-' + i);
  if (btn) btn.disabled = false;
  if (result.found && result.ip) {
    editorDevices[i].ip = result.ip;
    const ipInput = document.getElementById('device-ip-' + i);
    if (ipInput) ipInput.value = result.ip;
    onDeviceIpChange(i);
    maybeClearConfigError();
    pulseMdnsCheckButton(i);
  } else {
    showMdnsNotFound(i);
  }
};

function requestDevicePing(ips, manual) {
  const list = ips.map(ip => (ip || '').trim()).filter(Boolean);
  if (!list.length) return;
  pingInFlight = true;
  list.forEach(ip => {
    if (manual || devicePingStatus[ip] !== 'online') {
      devicePingStatus[ip] = 'checking';
    }
  });
  updatePingBadges();
  window.ipc.postMessage('ping-devices:' + JSON.stringify({ ips: list }));
}

function startDevicePingLoop() {
  if (pingPollTimer) clearInterval(pingPollTimer);
  pingAllDevices();
  pingPollTimer = setInterval(() => {
    if (!pingInFlight) pingAllDevices();
  }, PING_POLL_MS);
}

function pingAllDevices() {
  requestDevicePing(editorDevices.map(d => d.ip), false);
}

function pingDevice(index, manual) {
  const ip = (editorDevices[index] && editorDevices[index].ip || '').trim();
  if (!ip) {
    setConfigStatus('Enter an IP address to ping.', true);
    return;
  }
  requestDevicePing([ip], !!manual);
}

function onDeviceIpChange(index) {
  if (pingDebounceTimer) clearTimeout(pingDebounceTimer);
  pingDebounceTimer = setTimeout(() => pingDevice(index), 700);
}

window.onDevicePingResults = function(payload) {
  pingInFlight = false;
  (payload.results || []).forEach(r => {
    if (!r.ip || r.known === false) return;
    devicePingStatus[r.ip] = r.online ? 'online' : 'offline';
  });
  updatePingBadges();
};

function sliderValueFromEvent(trackEl, ev) {
  const rect = trackEl.getBoundingClientRect();
  const y = (ev.clientY ?? (ev.touches && ev.touches[0] && ev.touches[0].clientY) ?? rect.bottom) - rect.top;
  return 1 - Math.max(0, Math.min(1, y / rect.height));
}

function setSliderVisual(trackEl, value) {
  value = Math.max(0, Math.min(1, value));
  const fill = trackEl.querySelector('.test-slider-fill');
  if (fill) fill.style.height = Math.round(value * 100) + '%';
  trackEl.classList.toggle('active', value > 0);
  const arrow = trackEl.querySelector('.test-slider-arrow');
  if (arrow) {
    const trackH = trackEl.clientHeight || 1;
    const arrowZonePx = 48;
    arrow.classList.toggle('hidden', value > 0 && value * trackH >= arrowZonePx);
  }
}

function setMotorSquareVisual(squareEl, value) {
  value = Math.max(0, Math.min(1, value));
  squareEl.style.setProperty('--motor-level', String(value));
  squareEl.classList.toggle('active', value > 0);
}

function setMotorVisualForDevice(index, value) {
  const card = document.querySelector('.device-card[data-device-index="' + index + '"]');
  if (!card) return;
  const track = card.querySelector('.test-slider-track');
  if (track) setSliderVisual(track, value);
  const square = card.querySelector('.motor-indicator-square');
  if (square) setMotorSquareVisual(square, value);
}

function endActiveTestDrag() {
  if (!activeTestDrag) return;
  const drag = activeTestDrag;
  activeTestDrag = null;
  drag.dragEnded = true;
  if (drag.pendingSend !== null) {
    cancelAnimationFrame(drag.pendingSend);
    drag.pendingSend = null;
  }
  if (drag.onMove) drag.trackEl.removeEventListener('pointermove', drag.onMove);
  if (drag.onLostCapture) {
    drag.trackEl.removeEventListener('lostpointercapture', drag.onLostCapture);
  }
  try {
    if (drag.trackEl && drag.pointerId != null) {
      drag.trackEl.releasePointerCapture(drag.pointerId);
    }
  } catch (_) {}
  if (drag.trackEl) {
    const idx = parseInt(drag.trackEl.dataset.index, 10);
    if (!Number.isNaN(idx)) setMotorVisualForDevice(idx, 0);
    else setSliderVisual(drag.trackEl, 0);
  }
  if (drag.ip) {
    window.ipc.postMessage('device-motor:' + JSON.stringify({ ip: drag.ip, value: 0 }));
    window.ipc.postMessage('device-stop:' + drag.ip);
  }
}

function setupTestSliderSafety() {
  if (testSliderSafetyReady) return;
  testSliderSafetyReady = true;
  window.addEventListener('pointerup', (e) => {
    if (activeTestDrag && activeTestDrag.pointerId === e.pointerId) {
      endActiveTestDrag();
    }
  }, true);
  window.addEventListener('pointercancel', (e) => {
    if (activeTestDrag && activeTestDrag.pointerId === e.pointerId) {
      endActiveTestDrag();
    }
  }, true);
  window.addEventListener('blur', () => endActiveTestDrag(), true);
  document.addEventListener('visibilitychange', () => {
    if (document.hidden) endActiveTestDrag();
  });
}

function bindDeviceSliders() {
  document.querySelectorAll('.test-slider-track').forEach((trackEl) => {
    const index = parseInt(trackEl.dataset.index, 10);
    trackEl.onpointerdown = (e) => {
      e.preventDefault();
      endActiveTestDrag();
      trackEl.setPointerCapture(e.pointerId);
      beginSliderDrag(index, trackEl, e);
    };
  });
  document.querySelectorAll('.motor-indicator-square').forEach((squareEl) => {
    const index = parseInt(squareEl.dataset.index, 10);
    squareEl.onpointerdown = (e) => {
      const card = squareEl.closest('.device-card');
      if (!card || !card.classList.contains('is-collapsed')) return;
      e.preventDefault();
      endActiveTestDrag();
      squareEl.setPointerCapture(e.pointerId);
      beginMotorSquarePress(index, squareEl, e);
    };
  });
}

function beginMotorSquarePress(index, squareEl, e) {
  const ip = (editorDevices[index] && editorDevices[index].ip || '').trim();
  if (!ip) {
    setConfigStatus('Enter an IP address before testing.', true);
    try { squareEl.releasePointerCapture(e.pointerId); } catch (_) {}
    return;
  }

  const drag = {
    pointerId: e.pointerId,
    trackEl: squareEl,
    ip,
    dragEnded: false,
    pendingSend: null,
    lastSent: 100,
    onMove: null,
    onLostCapture: null,
  };
  activeTestDrag = drag;

  setMotorVisualForDevice(index, 1);
  window.ipc.postMessage('device-motor:' + JSON.stringify({ ip: ip, value: 1 }));

  drag.onLostCapture = () => endActiveTestDrag();
  squareEl.addEventListener('lostpointercapture', drag.onLostCapture);
}

function beginSliderDrag(index, trackEl, e) {
  const ip = (editorDevices[index] && editorDevices[index].ip || '').trim();
  if (!ip) {
    setConfigStatus('Enter an IP address before testing.', true);
    try { trackEl.releasePointerCapture(e.pointerId); } catch (_) {}
    return;
  }

  const drag = {
    pointerId: e.pointerId,
    trackEl,
    ip,
    dragEnded: false,
    pendingSend: null,
    lastSent: -1,
    onMove: null,
    onLostCapture: null,
  };
  activeTestDrag = drag;

  const postMotor = (value) => {
    window.ipc.postMessage('device-motor:' + JSON.stringify({ ip: ip, value: value }));
  };

  drag.onMove = (ev) => {
    if (drag.dragEnded || activeTestDrag !== drag) return;
    const value = sliderValueFromEvent(trackEl, ev);
    setMotorVisualForDevice(index, value);
    const stepped = Math.round(value * 100);
    if (stepped === drag.lastSent) return;
    drag.lastSent = stepped;
    postMotor(stepped / 100);
  };

  drag.onLostCapture = () => endActiveTestDrag();

  drag.onMove(e);
  trackEl.addEventListener('pointermove', drag.onMove);
  trackEl.addEventListener('lostpointercapture', drag.onLostCapture);
}

function applyMaxSpeedToDeviceUi(index, speed) {
  const powerMin = editorSpeedDefaults.min || 5;
  speed = Math.max(powerMin, Math.min(100, Math.round(speed)));
  if (!editorDevices[index]) return;
  editorDevices[index].max_speed = speed;
  const slider = document.getElementById('max-speed-slider-' + index);
  if (slider) slider.value = Math.round(speedToSliderPos(speed) * SPEED_SLIDER_STEPS);
  const label = document.getElementById('max-speed-val-' + index);
  if (label) label.textContent = speed + '%';
}

function onMaxSpeedChange(index, input) {
  const t = parseInt(input.value, 10) / SPEED_SLIDER_STEPS;
  applyMaxSpeedToDeviceUi(index, sliderPosToSpeed(t));
}

window.onMaxSpeedFromVrc = function(payload) {
  if (!payload || !payload.ip) return;
  const ip = String(payload.ip).trim();
  const speed = payload.max_speed;
  if (speed == null || isNaN(speed)) return;
  const idx = editorDevices.findIndex((d) => (d.ip || '').trim() === ip);
  if (idx < 0) return;
  applyMaxSpeedToDeviceUi(idx, speed);
};

function togglePanelInfo(event, id) {
  if (event) {
    event.preventDefault();
    event.stopPropagation();
  }
  const el = document.getElementById(id);
  if (!el) return;
  const show = el.classList.contains('hidden');
  document.querySelectorAll('.panel-info-text').forEach((node) => node.classList.add('hidden'));
  document.querySelectorAll('.panel-info-btn').forEach((btn) => btn.setAttribute('aria-expanded', 'false'));
  if (show) {
    el.classList.remove('hidden');
    const btn = event && event.currentTarget;
    if (btn) btn.setAttribute('aria-expanded', 'true');
  }
}

function onVelocityControlChange(index, input) {
  if (!editorDevices[index]) return;
  const enabling = !!input.checked;
  editorDevices[index].use_velocity_control = enabling;
  if (enabling && !hasHeadpatPanelPreference(index)) {
    setHeadpatPanelVisible(index, true);
  }
  syncColliderViz(index);
  renderDevices();
  saveConfig(true);
}

function toggleColliderAdjustment(index) {
  const visible = !isColliderAdjustmentVisible(index);
  setColliderAdjustmentVisible(index, visible);
  const body = document.getElementById('proximity-band-' + index);
  if (body) body.classList.toggle('hidden', !visible);
  const btn = document.getElementById('collider-adjust-toggle-' + index);
  if (btn) {
    btn.setAttribute('aria-expanded', visible ? 'true' : 'false');
    btn.setAttribute('aria-label', (visible ? 'Hide' : 'Show') + ' collider adjustment for device ' + (index + 1));
  }
}

function toggleDeviceSetup(index) {
  const visible = !isDeviceSetupVisible(index);
  setDeviceSetupVisible(index, visible);
  const body = document.getElementById('device-setup-' + index);
  if (body) body.classList.toggle('hidden', !visible);
  const btn = document.getElementById('device-setup-toggle-' + index);
  if (btn) {
    btn.setAttribute('aria-expanded', visible ? 'true' : 'false');
    btn.setAttribute('aria-label', (visible ? 'Hide' : 'Show') + ' device setup for device ' + (index + 1));
  }
  syncLogSectionLayout();
}

function togglePowerPanel(index) {
  const visible = !isPowerPanelVisible(index);
  setPowerPanelVisible(index, visible);
  const body = document.getElementById('power-panel-' + index);
  if (body) body.classList.toggle('hidden', !visible);
  const val = document.getElementById('max-speed-val-' + index);
  if (val) val.style.display = visible ? '' : 'none';
  const btn = document.getElementById('power-panel-toggle-' + index);
  if (btn) {
    btn.setAttribute('aria-expanded', visible ? 'true' : 'false');
    btn.setAttribute('aria-label', (visible ? 'Hide' : 'Show') + ' power for device ' + (index + 1));
  }
  syncLogSectionLayout();
}

function toggleHeadpatPanel(index) {
  if (!editorDevices[index] || !editorDevices[index].use_velocity_control) return;
  const visible = !isHeadpatPanelVisible(index);
  setHeadpatPanelVisible(index, visible);
  const body = document.getElementById('headpat-panel-' + index);
  if (body) body.classList.toggle('hidden', !visible);
  const btn = document.getElementById('headpat-panel-toggle-' + index);
  if (btn) {
    btn.setAttribute('aria-expanded', visible ? 'true' : 'false');
    btn.setAttribute('aria-label', (visible ? 'Hide' : 'Show') + ' headpat settings for device ' + (index + 1));
  }
  syncLogSectionLayout();
}

function toggleDeviceCardCollapse(index) {
  const collapsed = !isDeviceCardCollapsed(index);
  setDeviceCardCollapsed(index, collapsed);
  applyDeviceCardCollapsedUi(index, collapsed);
  if (collapsed) endActiveTestDrag();
  syncLogSectionLayout();
}

function onVelocityOnProxDropChange(index, input) {
  if (!editorDevices[index]) return;
  editorDevices[index].velocity_on_prox_drop = !!input.checked;
  syncColliderViz(index);
  saveConfig(true);
}

/** Inner slider: close edge (`inner_proximity`, maximum of active band). */
function onInnerProxBandChange(index, input) {
  const d = editorDevices[index];
  if (!d) return;
  let closeEdge = colliderSliderPctToProx(parseInt(input.value, 10));
  const farEdge = effectiveOuterProx(d);
  if (closeEdge <= farEdge) closeEdge = Math.min(1, farEdge + 0.01);
  d.inner_proximity = closeEdge;
  input.value = proxToColliderSliderPct(closeEdge);
  syncColliderViz(index);
  maybeClearConfigError();
}

/** Outer slider: far edge (`outer_proximity`, minimum of active band). */
function onOuterProxBandChange(index, input) {
  const d = editorDevices[index];
  if (!d) return;
  const farEdge = colliderSliderPctToProx(parseInt(input.value, 10));
  d.outer_proximity = farEdge;
  if (d.inner_proximity <= farEdge) {
    const closeEdge = Math.min(1, farEdge + 0.01);
    d.inner_proximity = closeEdge;
    const innerInput = document.getElementById('inner-prox-' + index);
    if (innerInput) innerInput.value = proxToColliderSliderPct(closeEdge);
  }
  syncColliderViz(index);
  maybeClearConfigError();
}

function onVelocityScalarChange(index, input) {
  const d = editorDevices[index];
  if (!d) return;
  const v = parseInt(input.value, 10);
  d.velocity_scalar = v;
  const label = document.getElementById('velocity-scalar-val-' + index);
  if (label) label.textContent = String(v) + '%';
  syncColliderViz(index);
  maybeClearConfigError();
}

function onVelocitySoftcapChange(index, input) {
  const d = editorDevices[index];
  if (!d) return;
  const dampingPct = parseInt(input.value, 10);
  d.velocity_softcap = dampingPctToVelocitySoftcap(dampingPct);
  const label = document.getElementById('velocity-softcap-val-' + index);
  if (label) label.textContent = String(dampingPct) + '%';
  syncColliderViz(index);
  maybeClearConfigError();
}

function onVelocitySmoothingChange(index, input) {
  const d = editorDevices[index];
  if (!d) return;
  const v = parseInt(input.value, 10);
  d.velocity_smoothing_ms = v;
  const label = document.getElementById('velocity-smoothing-val-' + index);
  if (label) label.textContent = String(v) + 'ms';
  syncColliderViz(index);
  maybeClearConfigError();
}

function addDevice() {
  editorDevices.push({
    name: editorDevices.length === 0 ? 'Headpats' : '',
    ip: '',
    proximity_parameter: 'proximity_01',
    max_speed_parameter: '',
    max_speed: editorSpeedDefaults.max,
    use_velocity_control: editorVelocityDefault,
    velocity_on_prox_drop: editorVelocityOnProxDropDefault,
    outer_proximity: editorVelocityProxDefaults.outer,
    inner_proximity: editorVelocityProxDefaults.inner,
    velocity_scalar: editorVelocityProxDefaults.scalar,
    velocity_softcap: editorVelocityProxDefaults.softcap,
    velocity_smoothing_ms: editorVelocityProxDefaults.smoothing_ms
  });
  renderDevices();
}

function requestRemoveDevice(index) {
  if (!editorDevices[index]) return;
  pendingRemoveIndex = index;
  renderDevices();
}

function confirmRemoveDevice(index) {
  if (pendingRemoveIndex !== index) return;
  editorDevices.splice(index, 1);
  pendingRemoveIndex = null;
  renderDevices();
}

function cancelRemoveDevice() {
  pendingRemoveIndex = null;
  renderDevices();
}

function isOscQueryPort() {
  return String(editorPortRx || '').trim().toLowerCase() === 'oscquery';
}

function updateOscPortUi() {
  const btn = document.getElementById('osc-mode-btn');
  const input = document.getElementById('osc-port-input');
  if (!btn || !input) return;
  if (isOscQueryPort()) {
    btn.textContent = 'OSC: Query';
    btn.classList.add('osc-query-active');
    input.style.display = 'none';
    input.disabled = true;
  } else {
    btn.textContent = 'OSC: Port';
    btn.classList.remove('osc-query-active');
    input.style.display = 'block';
    input.disabled = false;
    input.value = String(editorPortRx || '9001');
  }
}

function toggleOscMode() {
  if (isOscQueryPort()) {
    editorPortRx = '9001';
    updateOscPortUi();
    saveConfig(true);
    const input = document.getElementById('osc-port-input');
    if (input) {
      input.focus();
      input.select();
    }
  } else {
    editorPortRx = 'OSCQuery';
    updateOscPortUi();
    saveConfig(true);
  }
}

function commitOscPortInput() {
  const input = document.getElementById('osc-port-input');
  if (!input || isOscQueryPort()) return;
  const raw = input.value.trim();
  const port = raw === '' ? '9001' : raw;
  const n = parseInt(port, 10);
  if (!Number.isFinite(n) || n < 1 || n > 65535) {
    setConfigStatus('OSC port must be a number from 1 to 65535.', true);
    input.value = String(editorPortRx || '9001');
    return;
  }
  editorPortRx = String(n);
  input.value = editorPortRx;
  saveConfig(true);
}

function saveConfig(quiet) {
  window.ipc.postMessage('save-config:' + JSON.stringify({
    devices: editorDevices,
    min_speed: editorSpeedDefaults.min,
    max_speed_cap: editorSpeedDefaults.max,
    port_rx: editorPortRx,
    quiet: !!quiet
  }));
}

window.onConfigLoaded = function(state) {
  if (state.min_speed != null) editorSpeedDefaults.min = state.min_speed;
  if (state.max_speed_cap != null) editorSpeedDefaults.max = state.max_speed_cap;
  if (state.default_use_velocity_control != null) {
    editorVelocityDefault = !!state.default_use_velocity_control;
  }
  if (state.default_velocity_on_prox_drop != null) {
    editorVelocityOnProxDropDefault = !!state.default_velocity_on_prox_drop;
  }
  if (state.default_outer_proximity != null) {
    editorVelocityProxDefaults.outer = state.default_outer_proximity;
  }
  if (state.default_inner_proximity != null) {
    editorVelocityProxDefaults.inner = state.default_inner_proximity;
  }
  if (state.default_velocity_scalar != null) {
    editorVelocityProxDefaults.scalar = state.default_velocity_scalar;
  }
  if (state.default_velocity_softcap != null) {
    editorVelocityProxDefaults.softcap = state.default_velocity_softcap;
  }
  if (state.default_velocity_smoothing_ms != null) {
    editorVelocityProxDefaults.smoothing_ms = state.default_velocity_smoothing_ms;
  }
  if (state.port_rx != null) editorPortRx = String(state.port_rx);
  const powerMin = editorSpeedDefaults.min;
  editorDevices = (state.devices || []).map((d, i) => ({
    ...d,
    max_speed_parameter: (d.max_speed_parameter || '').trim(),
    max_speed: Math.max(powerMin, d.max_speed ?? powerMin),
    use_velocity_control: !!d.use_velocity_control,
    velocity_on_prox_drop: !!d.velocity_on_prox_drop,
    outer_proximity: d.outer_proximity ?? editorVelocityProxDefaults.outer,
    inner_proximity: d.inner_proximity ?? editorVelocityProxDefaults.inner,
    velocity_scalar: d.velocity_scalar ?? editorVelocityProxDefaults.scalar,
    velocity_softcap: d.velocity_softcap ?? editorVelocityProxDefaults.softcap,
    velocity_smoothing_ms: d.velocity_smoothing_ms ?? editorVelocityProxDefaults.smoothing_ms,
  }));
  renderDevices();
  updateOscPortUi();
  clearConfigStatus();
  startDevicePingLoop();
  fitStartupWindowHeight();
};

/** One-time startup height: one device card + footer (+ header). Width unchanged. */
function fitStartupWindowHeight() {
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      syncLogSectionLayout();
      const header = document.querySelector('header');
      const wrap = document.getElementById('config-wrap');
      const list = document.getElementById('device-list');
      const footer = document.querySelector('#config-wrap .config-footer');
      if (!wrap || !list || !footer) return;

      const headerH = header ? header.getBoundingClientRect().height : 0;
      const statusEl = document.getElementById('config-status');
      const statusH = (statusEl && statusEl.classList.contains('err'))
        ? statusEl.getBoundingClientRect().height : 0;
      const wrapStyle = getComputedStyle(wrap);
      const gap = parseFloat(wrapStyle.rowGap || wrapStyle.gap || '0') || 12;
      const wrapPadY = parseFloat(wrapStyle.paddingTop || '0')
        + parseFloat(wrapStyle.paddingBottom || '0');

      const firstCard = list.querySelector('.device-card');
      const hint = list.querySelector('.hint');
      const listStyle = getComputedStyle(list);
      const listPadY = parseFloat(listStyle.paddingTop || '0')
        + parseFloat(listStyle.paddingBottom || '0');
      let listContentH = 0;
      if (firstCard) {
        listContentH = firstCard.getBoundingClientRect().height;
      } else if (hint) {
        listContentH = hint.getBoundingClientRect().height;
      }

      const actionsBar = document.querySelector('.device-list-actions');
      const actionsH = actionsBar ? actionsBar.getBoundingClientRect().height : 0;
      const btnH = footer.getBoundingClientRect().height;
      const configColumnH = wrapPadY + statusH + listPadY + listContentH + actionsH + gap + btnH;
      const slack = 32;
      let h = Math.ceil(headerH + configColumnH + slack);

      if (isLogColumnOpen()) {
        h = Math.max(h, 720);
      } else {
        h = Math.max(h, 480);
      }
      h = Math.ceil(h * 1.25);

      window.ipc.postMessage('startup-height:' + JSON.stringify({ h }));
    });
  });
}

window.onConfigSaved = function(opts) {
  opts = opts || {};
  clearConfigStatus();
  if (!opts.quiet) {
    window.ipc.postMessage('load-config');
  }
};

window.onConfigError = function(msg) {
  setConfigStatus(msg, true);
};

const patBarByParam = {};
/** Live motor level per device IP (matches router `device_uri` / editor `ip`). */
const motorBarByIp = {};

function applyMotorBars(updates) {
  if (activeTestDrag) return;
  for (const ip in updates) {
    const target = Math.max(0, Math.min(1, updates[ip]));
    const ipKey = (ip || '').trim();
    if (!ipKey) continue;

    // Smooth motor output so the live indicator looks stable (EMA),
    // but snap hard to 0 on power-down so it never looks "stuck".
    if (target <= 0) {
      motorBarByIp[ipKey] = 0;
    } else if (target >= 0.999) {
      // If the real motor out is at full-scale, show it hitting the top.
      motorBarByIp[ipKey] = 1;
    } else {
      // Higher alpha responds faster; lower alpha looks smoother.
      const prev = (motorBarByIp[ipKey] != null) ? motorBarByIp[ipKey] : target;
      const alpha = (target > prev) ? 0.35 : 0.18;
      const value = prev + (target - prev) * alpha;
      motorBarByIp[ipKey] = value;
    }
    editorDevices.forEach((d, i) => {
      if ((d.ip || '').trim() !== ipKey) return;
      setMotorVisualForDevice(i, motorBarByIp[ipKey]);
    });
  }
}

function applyPatBars(updates) {
  for (const param in updates) {
    const paramKey = (param || '').trim();
    if (!paramKey) continue;
    const graph = updates[param];
    if (graph) patBarByParam[paramKey] = graph;
    else delete patBarByParam[paramKey];
  }
  renderPatBars();
}

function renderPatBars() {
  const el = document.getElementById('pat-bars');
  if (!el) return;
  const lines = [];
  const seen = new Set();
  editorDevices.forEach((d, i) => {
    const paramKey = (d.proximity_parameter || '').trim();
    if (!paramKey || seen.has(paramKey)) return;
    const graph = patBarByParam[paramKey];
    if (!graph) return;
    seen.add(paramKey);
    const label = (d.name || paramKey || ('Device ' + (i + 1))).trim();
    lines.push(label + ': |' + graph);
  });
  Object.keys(patBarByParam).sort().forEach((paramKey) => {
    if (seen.has(paramKey)) return;
    lines.push(paramKey + ': |' + patBarByParam[paramKey]);
  });
  el.textContent = lines.join('\n');
}

let lastStatusLines = [];
const STATUS_LINE_PX = 12 * 1.45;
const STATUS_MAX_LINES = 100;

function statusViewportLines() {
  const box = document.getElementById('log-box');
  if (!box) return 40;
  const pad = 20;
  return Math.max(4, Math.floor((box.clientHeight - pad) / STATUS_LINE_PX));
}

function renderStatus(lines) {
  const el = document.getElementById('log');
  if (!el) return;
  const head = lines.slice(0, statusViewportLines());
  el.textContent = head.join('\n');
}

function setStatusLines(lines) {
  lastStatusLines = lines.slice(-STATUS_MAX_LINES).reverse();
  renderStatus(lastStatusLines);
}

function appendStatusLines(lines) {
  if (!lines || !lines.length) return;
  lastStatusLines = lines.slice().reverse().concat(lastStatusLines);
  if (lastStatusLines.length > STATUS_MAX_LINES) {
    lastStatusLines = lastStatusLines.slice(0, STATUS_MAX_LINES);
  }
  renderStatus(lastStatusLines);
}

function setupPaneScroll(wrapId, scrollId) {
  const wrap = document.getElementById(wrapId);
  const scroll = document.getElementById(scrollId);
  if (!wrap || !scroll) return;
  wrap.addEventListener('wheel', (e) => {
    const max = scroll.scrollHeight - scroll.clientHeight;
    if (max <= 0) return;
    scroll.scrollTop = Math.max(0, Math.min(max, scroll.scrollTop + e.deltaY));
    e.preventDefault();
    e.stopPropagation();
  }, { passive: false, capture: true });
}

setupPaneScroll('config-wrap', 'config-scroll');
setupPaneScroll('log-section', 'log-cards-scroll');
setupTestSliderSafety();
const logBox = document.getElementById('log-box');
if (logBox) {
  new ResizeObserver(() => {
    if (lastStatusLines.length) renderStatus(lastStatusLines);
  }).observe(logBox);
}
const configScroll = document.getElementById('config-scroll');
if (configScroll) {
  configScroll.addEventListener('scroll', () => syncLogSectionLayout(), { passive: true });
}
window.addEventListener('resize', () => syncLogSectionLayout());
const configFooter = document.querySelector('#config-wrap .config-footer');
const deviceList = document.getElementById('device-list');
if (typeof ResizeObserver !== 'undefined') {
  if (configFooter) {
    new ResizeObserver(() => syncLogSectionLayout()).observe(configFooter);
  }
  if (deviceList) {
    new ResizeObserver(() => syncLogSectionLayout()).observe(deviceList);
  }
}
applyConsolePanelUi();
requestAnimationFrame(() => syncLogSectionLayout());
window.ipc.postMessage('load-config');
</script>
</body>
</html>"#;

fn output_html() -> String {
  use base64::{engine::general_purpose::STANDARD, Engine};
  let uri = format!(
    "data:image/png;base64,{}",
    STANDARD.encode(include_bytes!("assets/Giggletech_Black.png"))
  );
  let card_inner =
    serde_json::to_string(collider_viz::COLLIDER_VIZ_CARD_INNER).unwrap_or_else(|_| "\"\"".to_string());
  OUTPUT_HTML
    .replace(LOGO_PLACEHOLDER, &uri)
    .replace(COLLIDER_VIZ_STYLES_PLACEHOLDER, collider_viz::COLLIDER_VIZ_STYLES)
    .replace(COLLIDER_VIZ_RUNTIME_PLACEHOLDER, collider_viz::COLLIDER_VIZ_RUNTIME)
    .replace(COLLIDER_VIZ_CARD_INNER_PLACEHOLDER, &card_inner)
}

enum UserEvent {
  TrayIconEvent(tray_icon::TrayIconEvent),
  MenuEvent(tray_icon::menu::MenuEvent),
  StatusUpdated,
  LiveUiFlush,
  ColliderProxFlush,
  ConfigIpc(String),
  ColliderVizOpen(String),
  ColliderVizUpdate(String),
  ColliderVizClose(usize),
  PingResults(String),
  MdnsLookupResult(String),
  MaxSpeedFromVrc(String),
  ShowOutput,
}

#[derive(Debug, Deserialize)]
struct PingDevicesRequest {
  ips: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct MdnsCheckRequest {
  device_index: usize,
}

struct OutputWindow {
  window: Window,
  webview: wry::WebView,
}

const STATUS_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

fn webview_data_directory() -> PathBuf {
  let mut path = data_local_dir().expect("Failed to get LOCALAPPDATA directory");
  path.push("GiggleTech");
  path.push("WebView2");
  std::fs::create_dir_all(&path).expect("Failed to create WebView2 data directory");
  path
}

struct UiState {
  web_context: WebContext,
  output: Option<OutputWindow>,
  collider_viz_open: HashMap<usize, ColliderVizState>,
  status_synced: usize,
  status_epoch: usize,
  status_pending: bool,
  last_status_flush: Option<Instant>,
  startup_height_fitted: bool,
}

impl UiState {
  fn new() -> Self {
    Self {
      web_context: WebContext::new(Some(webview_data_directory())),
      output: None,
      collider_viz_open: HashMap::new(),
      status_synced: 0,
      status_epoch: log_ui::buffer_epoch(),
      status_pending: false,
      last_status_flush: None,
      startup_height_fitted: false,
    }
  }

  fn output_window_id(&self) -> Option<tao::window::WindowId> {
    self.output.as_ref().map(|o| o.window.id())
  }

  fn push_collider_viz_state(webview: &wry::WebView, state: &ColliderVizState) {
    let _ = webview.evaluate_script(&collider_viz::state_script(state));
  }

  fn flush_collider_live_to(
    webview: &wry::WebView,
    active: &ColliderVizState,
    prox_batch: &HashMap<String, f32>,
    headpat_batch: &HashMap<String, String>,
  ) {
    let active_key = collider_viz::batch_key(&active.device_ip, &active.proximity_parameter);
    let prox = prox_batch.get(&active_key).copied();
    let device_ip = active.device_ip.trim();
    let motor_live = (!device_ip.is_empty())
      .then(|| PENDING_MOTOR_BARS.lock().unwrap().get(device_ip).copied())
      .flatten();
    let fresh = headpat_batch.get(&active_key).cloned();
    let telemetry = fresh.clone().or_else(|| {
      LAST_HEADPAT_TELEMETRY
        .lock()
        .unwrap()
        .get(&active_key)
        .cloned()
    });
    if let Some(json) = telemetry {
      let append = fresh.is_some();
      let script = motor_live
        .map(|motor| merge_headpat_telemetry_motor(&json, motor))
        .unwrap_or(json);
      let script =
        collider_viz::collider_flush_script(active.index, prox, Some(&script), append);
      if !script.is_empty() {
        let _ = webview.evaluate_script(&script);
      }
    } else if let Some(p) = prox {
      let _ = webview.evaluate_script(&collider_viz::prox_sample_script(active.index, p));
    } else if let Some(motor) = motor_live {
      let _ = webview.evaluate_script(&collider_viz::headpat_motor_script(active.index, motor));
    }
  }

  fn flush_collider_live(&mut self) {
    if self.collider_viz_open.is_empty() {
      return;
    }
    let Some(output) = &self.output else {
      return;
    };
    let prox_batch: HashMap<String, f32> = PENDING_PROX_SIGNALS.lock().unwrap().drain().collect();
    let headpat_batch: HashMap<String, String> =
      PENDING_HEADPAT_TELEMETRY.lock().unwrap().drain().collect();
    for state in self.collider_viz_open.values() {
      Self::flush_collider_live_to(&output.webview, state, &prox_batch, &headpat_batch);
    }
  }

  fn show_collider_viz(&mut self, state: ColliderVizState) {
    self.collider_viz_open.insert(state.index, state.clone());
    if let Some(output) = &self.output {
      let json = serde_json::to_string(&state).unwrap_or_else(|_| "{}".to_string());
      let _ = output
        .webview
        .evaluate_script(&format!("openColliderVizCard({json});"));
      Self::push_collider_viz_state(&output.webview, &state);
    }
  }

  fn close_collider_viz(&mut self, index: usize) {
    self.collider_viz_open.remove(&index);
  }

  fn close_all_collider_viz(&mut self) {
    self.collider_viz_open.clear();
    if let Some(output) = &self.output {
      let _ = output.webview.evaluate_script(
        "(function(){var list=document.getElementById('log-viz-cards');\
         if(!list)return;\
         list.querySelectorAll('.log-viz-card').forEach(function(c){\
           var i=parseInt(c.dataset.vizIndex,10);\
           if(window.colliderVizApi)window.colliderVizApi.unmount(i);\
         });\
         list.innerHTML='';})();",
      );
    }
  }

  fn create_output_window(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    ipc_proxy: tao::event_loop::EventLoopProxy<UserEvent>,
  ) {
    let app_icon = load_tao_icon_from_ico(32);
    let window = WindowBuilder::new()
      .with_title("GiggleTech")
      .with_inner_size(LogicalSize::new(OUTPUT_WINDOW_WIDTH, OUTPUT_WINDOW_HEIGHT))
      .with_min_inner_size(LogicalSize::new(
        OUTPUT_WINDOW_MIN_WIDTH,
        OUTPUT_WINDOW_MIN_HEIGHT,
      ))
      .with_window_icon(Some(app_icon))
      .build(event_loop)
      .expect("Failed to create output window");

    let webview = WebViewBuilder::with_web_context(&mut self.web_context)
      .with_html(output_html())
      .with_ipc_handler(move |request: Request<String>| {
        let _ = ipc_proxy.send_event(UserEvent::ConfigIpc(request.body().clone()));
      })
      .build(&window)
      .expect("Failed to create output webview");

    self.status_synced = 0;
    self.status_epoch = log_ui::buffer_epoch();
    self.status_pending = false;
    sync_status_to_webview(&webview, &mut self.status_synced, &mut self.status_epoch);

    self.output = Some(OutputWindow { window, webview });
  }

  fn show_output(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    ipc_proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
  ) {
    // If another instance (or the OS) repeatedly signals "show output", avoid
    // re-showing / refocusing the window which can feel like "window spam".
    if self.is_output_visible() {
      if let Some(output) = &self.output {
        sync_status_to_webview(&output.webview, &mut self.status_synced, &mut self.status_epoch);
        self.status_pending = false;
        self.last_status_flush = Some(Instant::now());
      }
      return;
    }

    if self.output.is_none() {
      self.create_output_window(event_loop, ipc_proxy.clone());
    }

    if let Some(output) = &self.output {
      output.window.set_visible(true);
      output.window.set_focus();
      sync_status_to_webview(&output.webview, &mut self.status_synced, &mut self.status_epoch);
      self.status_pending = false;
      self.last_status_flush = Some(Instant::now());
      flush_live_ui(&output.webview);
      for state in self.collider_viz_open.values() {
        let json = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
        let _ = output.webview.evaluate_script(&format!(
          "if(typeof openColliderVizCard==='function')openColliderVizCard({json});"
        ));
      }
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

  fn on_status_updated(&mut self) {
    if !self.is_output_visible() {
      self.status_pending = true;
      return;
    }
    let now = Instant::now();
    if self
      .last_status_flush
      .is_some_and(|t| now.duration_since(t) < STATUS_FLUSH_INTERVAL)
    {
      self.status_pending = true;
      return;
    }
    if let Some(output) = &self.output {
      flush_status_to_webview(
        &output.webview,
        &mut self.status_synced,
        &mut self.status_epoch,
      );
      self.status_pending = false;
      self.last_status_flush = Some(now);
    }
  }

  fn flush_pending_status(&mut self) {
    if !self.status_pending || !self.is_output_visible() {
      return;
    }
    if let Some(output) = &self.output {
      flush_status_to_webview(
        &output.webview,
        &mut self.status_synced,
        &mut self.status_epoch,
      );
      self.status_pending = false;
      self.last_status_flush = Some(Instant::now());
    }
  }

  fn close_output(&mut self) {
    self.output = None;
    self.status_synced = 0;
    self.status_epoch = log_ui::buffer_epoch();
    self.status_pending = false;
    self.last_status_flush = None;
  }

  fn close_all_windows(&mut self) {
    self.close_output();
    self.close_all_collider_viz();
  }
}

fn sync_status_to_webview(
  webview: &wry::WebView,
  synced: &mut usize,
  epoch: &mut usize,
) {
  let lines = log_ui::snapshot();
  *synced = lines.len();
  *epoch = log_ui::buffer_epoch();
  if let Ok(json) = serde_json::to_string(&lines) {
    let _ = webview.evaluate_script(&format!("setStatusLines({});", json));
  }
}

fn flush_status_to_webview(
  webview: &wry::WebView,
  synced: &mut usize,
  epoch: &mut usize,
) {
  let current_epoch = log_ui::buffer_epoch();
  if current_epoch != *epoch {
    sync_status_to_webview(webview, synced, epoch);
    return;
  }
  let (new_lines, len) = log_ui::lines_since(*synced);
  *synced = len;
  if new_lines.is_empty() {
    return;
  }
  if let Ok(json) = serde_json::to_string(&new_lines) {
    let _ = webview.evaluate_script(&format!("appendStatusLines({});", json));
  }
}

fn handle_config_ipc(
  webview: &wry::WebView,
  event_proxy: &EventLoopProxy<UserEvent>,
  msg: &str,
) {
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
  } else if msg == "autostart-get" {
    let enabled = is_auto_start_enabled();
    log_ui::status(&format!("Auto-start is {}.", if enabled { "ON" } else { "OFF" }));
    let _ = webview.evaluate_script(&format!(
      "window.onAutoStartState({{ enabled: {} }});",
      if enabled { "true" } else { "false" }
    ));
  } else if let Some(flag) = msg.strip_prefix("autostart-set:") {
    let want_enabled = flag.trim() == "1" || flag.trim().eq_ignore_ascii_case("true");
    log_ui::status(&format!(
      "Updating auto-start → {}...",
      if want_enabled { "ON" } else { "OFF" }
    ));
    match set_auto_start(want_enabled) {
      Ok(_) => {
        log_ui::status("Auto-start updated.");
        let _ = webview.evaluate_script(&format!(
          "window.onAutoStartState({{ enabled: {} }});",
          if want_enabled { "true" } else { "false" }
        ));
      }
      Err(e) => {
        log_ui::status(&format!("Failed to update auto-start: {}", e));
        let err = serde_json::to_string(&format!("Failed to update auto-start: {}", e))
          .unwrap_or_else(|_| "\"Failed to update auto-start\"".to_string());
        let enabled = is_auto_start_enabled();
        let _ = webview.evaluate_script(&format!(
          "window.onAutoStartState({{ enabled: {}, error: {} }});",
          if enabled { "true" } else { "false" },
          err
        ));
      }
    }
  } else if let Some(json) = msg.strip_prefix("save-config:") {
    match config_editor::save_editor_json(json) {
      Ok(quiet) => {
        let _ = webview.evaluate_script(&format!(
          "window.onConfigSaved({{ quiet: {} }});",
          if quiet { "true" } else { "false" }
        ));
      }
      Err(e) => {
        let err = serde_json::to_string(&e).unwrap_or_else(|_| "\"Unknown error\"".to_string());
        let _ = webview.evaluate_script(&format!("window.onConfigError({});", err));
      }
    }
  } else if let Some(json) = msg.strip_prefix("device-motor:") {
    if let Ok(payload) = serde_json::from_str::<device_test::MotorPayload>(json) {
      device_test::set_device_motor(payload.ip, payload.value);
    }
  } else if let Some(ip) = msg.strip_prefix("device-stop:") {
    device_test::stop_device(ip.trim().to_string());
  } else if let Some(json) = msg.strip_prefix("ping-devices:") {
    let req: PingDevicesRequest = match serde_json::from_str(json) {
      Ok(r) => r,
      Err(_) => return,
    };
    let ping_monitor = device_ping::monitor();
    ping_monitor.sync_ips(&req.ips);
    let results = ping_monitor.snapshot_for_ips(&req.ips);
    if let Ok(payload) = serde_json::to_string(&serde_json::json!({ "results": results })) {
      let _ = event_proxy.send_event(UserEvent::PingResults(payload));
    }
  } else if let Some(json) = msg.strip_prefix("mdns-check:") {
    let req: MdnsCheckRequest = match serde_json::from_str(json) {
      Ok(r) => r,
      Err(_) => return,
    };
    let proxy = event_proxy.clone();
    std::thread::spawn(move || {
      let result = device_discovery::lookup_giggletech_webpage(req.device_index);
      if result.found {
        log_ui::status("Found device.");
      }
      let payload = serde_json::to_string(&result).unwrap_or_else(|_| "{}".to_string());
      let _ = proxy.send_event(UserEvent::MdnsLookupResult(payload));
    });
  } else if let Some(json) = msg.strip_prefix("collider-viz-open:") {
    let _ = event_proxy.send_event(UserEvent::ColliderVizOpen(json.to_string()));
  } else if let Some(json) = msg.strip_prefix("collider-viz-update:") {
    let _ = event_proxy.send_event(UserEvent::ColliderVizUpdate(json.to_string()));
  } else if let Some(index_str) = msg.strip_prefix("collider-viz-close:") {
    if let Ok(index) = index_str.trim().parse::<usize>() {
      let _ = event_proxy.send_event(UserEvent::ColliderVizClose(index));
    }
  }
}

/// Run the tray icon event loop on the main thread. Blocks until the user exits.
///
/// When `start_minimized` is true (Windows login / `--autostart`), only the tray icon
/// is shown until the user opens the output window.
pub fn run(start_minimized: bool, primary: PrimaryInstance) {
  let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
  let event_proxy = event_loop.create_proxy();
  let ipc_proxy = event_loop.create_proxy();

  let show_proxy = event_loop.create_proxy();
  primary.spawn_show_listener(move || {
    let _ = show_proxy.send_event(UserEvent::ShowOutput);
  });

  log_ui::set_status_notify(move || {
    let _ = event_proxy.send_event(UserEvent::StatusUpdated);
  });

  let motor_proxy = event_loop.create_proxy();
  log_ui::set_proximity_notify(move |device_ip, value| {
    queue_motor_bar(device_ip.to_string(), value, &motor_proxy);
  });

  let pat_proxy = event_loop.create_proxy();
  log_ui::set_pat_bar_notify(move |param, graph| {
    queue_pat_bar(param.to_string(), graph.to_string(), &pat_proxy);
  });

  let prox_signal_proxy = event_loop.create_proxy();
  log_ui::set_prox_signal_notify(move |device_ip, param, value| {
    queue_prox_signal(device_ip.to_string(), param.to_string(), value, &prox_signal_proxy);
  });

  let headpat_proxy = event_loop.create_proxy();
  log_ui::set_headpat_telemetry_notify(move |device_ip, param, json| {
    queue_headpat_telemetry(
      device_ip.to_string(),
      param.to_string(),
      json.to_string(),
      &headpat_proxy,
    );
  });

  let max_speed_proxy = event_loop.create_proxy();
  log_ui::set_max_speed_notify(move |device_ip, percent| {
    if let Ok(json) = serde_json::to_string(&serde_json::json!({
      "ip": device_ip,
      "max_speed": percent,
    })) {
      let _ = max_speed_proxy.send_event(UserEvent::MaxSpeedFromVrc(json));
    }
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

        if is_auto_start_enabled() {
          let _ = set_auto_start(true);
        }

        if !start_minimized {
          ui_state.show_output(event_loop, &ipc_proxy);
        }
      }

      Event::UserEvent(UserEvent::StatusUpdated) => {
        ui_state.on_status_updated();
      }

      Event::MainEventsCleared => {
        ui_state.flush_pending_status();
      }

      Event::UserEvent(UserEvent::LiveUiFlush) => {
        if let Some(output) = &ui_state.output {
          if ui_state.is_output_visible() {
            flush_live_ui(&output.webview);
          } else {
            // Window hidden to tray: keep queued values but allow new flush events.
            LIVE_UI_FLUSH_PENDING.store(false, Ordering::Release);
          }
        } else {
          LIVE_UI_FLUSH_PENDING.store(false, Ordering::Release);
        }
      }

      Event::UserEvent(UserEvent::ConfigIpc(msg)) => {
        if let Some(output) = &ui_state.output {
          if let Some(json) = msg.strip_prefix("startup-height:") {
            if !ui_state.startup_height_fitted {
              if let Ok(req) = serde_json::from_str::<StartupHeightRequest>(json) {
                let scale = output.window.scale_factor();
                let current_physical = output.window.inner_size();
                let current_logical: LogicalSize<f64> = current_physical.to_logical(scale);
                let target_h = clamp_startup_height(req.h);
                output
                  .window
                  .set_inner_size(LogicalSize::new(current_logical.width, target_h));
                ui_state.startup_height_fitted = true;
              }
            }
          } else {
            handle_config_ipc(&output.webview, &ipc_proxy, &msg);
          }
        }
      }

      Event::UserEvent(UserEvent::PingResults(json)) => {
        if let Some(output) = &ui_state.output {
          let _ = output.webview.evaluate_script(&format!(
            "window.onDevicePingResults({});",
            json
          ));
        }
      }

      Event::UserEvent(UserEvent::MdnsLookupResult(json)) => {
        if let Some(output) = &ui_state.output {
          let _ = output.webview.evaluate_script(&format!(
            "window.onDeviceMdnsResult({});",
            json
          ));
        }
      }

      Event::UserEvent(UserEvent::MaxSpeedFromVrc(json)) => {
        if let Some(output) = &ui_state.output {
          let _ = output.webview.evaluate_script(&format!(
            "window.onMaxSpeedFromVrc({});",
            json
          ));
        }
      }

      Event::UserEvent(UserEvent::ColliderProxFlush) => {
        ui_state.flush_collider_live();
      }

      Event::UserEvent(UserEvent::ColliderVizOpen(json)) => {
        if let Some(state) = collider_viz::parse_state(&json) {
          ui_state.show_collider_viz(state);
        }
      }

      Event::UserEvent(UserEvent::ColliderVizUpdate(json)) => {
        if let Some(state) = collider_viz::parse_state(&json) {
          if ui_state.collider_viz_open.contains_key(&state.index) {
            ui_state.collider_viz_open.insert(state.index, state.clone());
            if let Some(output) = &ui_state.output {
              UiState::push_collider_viz_state(&output.webview, &state);
            }
          }
        }
      }

      Event::UserEvent(UserEvent::ColliderVizClose(index)) => {
        ui_state.close_collider_viz(index);
      }

      Event::UserEvent(UserEvent::ShowOutput) => {
        ui_state.show_output(event_loop, &ipc_proxy);
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
            log_ui::status(&format!("Failed to update auto-start: {}", e));
            auto_start.set_checked(!new_checked);
          }
        } else if menu_event.id == exit_item.id() {
          tray_icon.take();
          ui_state.close_all_windows();
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
  let (rgba, width, height) = load_rgba_from_ico(16).expect("Failed to decode src/assets/bolt.ico");
  tray_icon::Icon::from_rgba(rgba, width, height).expect("Failed to create tray icon image")
}

fn load_tao_icon_from_ico(size: u32) -> TaoIcon {
  let (rgba, width, height) = load_rgba_from_ico(size).expect("Failed to decode src/assets/bolt.ico");
  TaoIcon::from_rgba(rgba, width, height).expect("Failed to create app icon image")
}

fn load_rgba_from_ico(size: u32) -> Result<(Vec<u8>, u32, u32), String> {
  let bytes = include_bytes!("assets/bolt.ico");
  let img =
    image::load_from_memory(bytes).map_err(|e| format!("decode src/assets/bolt.ico: {}", e))?;
  let rgba = img.to_rgba8();

  if rgba.width() == size && rgba.height() == size {
    return Ok((rgba.into_raw(), size, size));
  }

  let resized = image::imageops::resize(
    &rgba,
    size,
    size,
    image::imageops::FilterType::Nearest,
  );
  Ok((resized.into_raw(), size, size))
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
    let quoted = format!("\"{}\" {}", exe.display(), AUTOSTART_ARG);
    run.set_value(AUTO_START_VALUE_NAME, &quoted)?;
  } else {
    let _ = run.delete_value(AUTO_START_VALUE_NAME);
  }

  Ok(())
}

fn current_exe_path() -> io::Result<PathBuf> {
  std::env::current_exe()
}
