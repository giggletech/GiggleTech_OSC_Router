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
const OUTPUT_WINDOW_WIDTH: f64 = 1080.0;
const OUTPUT_WINDOW_HEIGHT: f64 = 720.0;
const OUTPUT_WINDOW_MIN_WIDTH: f64 = 960.0;
const OUTPUT_WINDOW_MIN_HEIGHT: f64 = 480.0;
/// Same width as the device column in the output window (`#main` is two equal columns).
const COLLIDER_VIZ_WIDTH: f64 = OUTPUT_WINDOW_WIDTH / 2.0;
/// Taller than the output window so ring + chart fit without clipping the chart.
const COLLIDER_VIZ_HEIGHT: f64 = OUTPUT_WINDOW_HEIGHT + 100.0;
const COLLIDER_VIZ_MIN_WIDTH: f64 = 280.0;
const COLLIDER_VIZ_MIN_HEIGHT: f64 = 420.0;

fn clamp_startup_height(h: f64) -> f64 {
  // Per request: don't cap to monitor height; just ensure we don't go below the minimum.
  h.max(OUTPUT_WINDOW_MIN_HEIGHT)
}

static PENDING_MOTOR_BARS: Lazy<Mutex<HashMap<String, f32>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_PAT_BARS: Lazy<Mutex<HashMap<String, String>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_PROX_SIGNALS: Lazy<Mutex<HashMap<String, f32>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_HEADPAT_TELEMETRY: Lazy<Mutex<HashMap<String, String>>> =
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

fn queue_prox_signal(param: String, value: f32, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_PROX_SIGNALS
    .lock()
    .unwrap()
    .insert(collider_viz::param_key(&param), value);
  let _ = proxy.send_event(UserEvent::ColliderProxFlush);
}

fn queue_headpat_telemetry(param: String, json: String, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_HEADPAT_TELEMETRY
    .lock()
    .unwrap()
    .insert(collider_viz::param_key(&param), json);
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
  width: 100%;
  max-width: 1080px;
  min-height: 112px;
}
body.ui-large .header-inner { max-width: 2160px; }
.header-config-col {
  display: flex;
  justify-content: center;
  align-items: center;
  min-width: 0;
}
.header-logo {
  display: block;
  height: 104px;
  width: auto;
  max-width: 100%;
  object-fit: contain;
  object-position:  center;
  padding: 16px 16px 0px 16px;
}
.header-log-col {
  min-width: 0;
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 12px;
  /* Match right inset of #log-box (log-section padding + log-scroll padding). */
  padding: 16px 32px 0 16px;
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
  direction: rtl;
}
#device-list,
#device-list .device-card {
  direction: ltr;
}
#config-wrap .btn-row,
#config-wrap > .hint {
  flex-shrink: 0;
}
#config-wrap .btn-row {
  /* Match #device-list inset so buttons line up with card edges. */
  padding-left: 70px;
  padding-right: 6px;
  padding-bottom: 40px;
  padding-top: 32px;
  justify-content: flex-start;
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
#log-scroll {
  flex: 1 1 0;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  box-sizing: border-box;
  padding-right: 16px;
  background: transparent;
  border-radius: 0;
  border: none;
}
#log-bottom-spacer {
  flex-shrink: 0;
  width: 100%;
  box-sizing: border-box;
  padding-bottom: 16px;
}
#log-box {
  display: flex;
  flex-direction: column;
  width: 100%;
  height: 100%;
  background: #16161e;
  border-radius: 10px;
  border: 1px solid #2a2a36;
  padding: 10px 12px;
  box-sizing: border-box;
  overflow: hidden;
}
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
.device-card.is-collapsed .device-actions > .btn-danger {
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
  gap: 20px;
  flex-wrap: wrap;
}
.device-name-row .device-name-input {
  flex: 1;
  min-width: 16rem;
  margin-bottom: 0;
}
.device-status {
  flex-shrink: 0;
  font-size: 1.6rem;
  font-weight: 600;
  padding: 8px 20px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  line-height: 1;
  border-radius: 999px;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
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
.device-name-row .btn-sm {
  /* Match the one-line status pill styling/size. */
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 20px;
  border-radius: 999px;
  border: 2px solid #3f3f4e;
  background: #0f0f14;
  color: #b8b8c8;
  font-weight: 600;
  line-height: 1;
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
.proximity-band-hide-row {
  flex-shrink: 0;
  display: flex;
  gap: 12px;
  align-items: center;
}
.proximity-band-hide-row .btn-sm {
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
.proximity-band-hide-row .btn-sm:hover {
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
.device-actions { display: flex; gap: 16px; flex-wrap: wrap; align-items: center; margin-top: 32px; width: 100%; }
.device-actions .device-card-toggle-btn { margin-left: auto; }
.device-actions .btn[disabled] { opacity: 0.55; cursor: default; }
.device-actions .btn-sm {
  /* Make Confirm/Cancel match pill sizing. */
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 20px;
  border-radius: 999px;
  font-size: 1.6rem;
  font-weight: 600;
  line-height: 1;
}
.device-actions .btn-secondary.btn-sm {
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
}
.device-actions .btn-secondary.btn-sm:hover {
  background: #3f3f4e;
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
  /* Match the status/ping pill sizing in the device header row. */
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 8px 20px;
  border-radius: 999px;
  font-size: 1.6rem;
  font-weight: 600;
  line-height: 1;
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #b8b8c8;
}
.device-actions .btn-danger:hover {
  background: #3f3f4e;
}
.btn-row { display: flex; gap: 16px; flex-wrap: wrap; }
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
.btn-row .btn-primary {
  margin-left: auto;
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
  width: 11rem;
  flex-shrink: 0;
  padding: 18px 24px;
  font-size: 1.7rem;
  font-weight: 600;
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

#autostart-btn {
  padding: 12px 18px;
  font-size: 1.35rem;
  border: 2px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
}
#autostart-btn:hover { background: #3f3f4e; }
#autostart-btn.autostart-on { border-color: #a855f7; }
#autostart-btn:disabled { opacity: 0.75; cursor: default; }
</style>
</head>
<body>
<header>
  <div class="header-inner">
    <div class="header-config-col">
      <img class="header-logo" src="{{LOGO_URI}}" alt="GiggleTech">
    </div>
    <div class="header-log-col">
      <button type="button" class="btn btn-secondary" id="autostart-btn" aria-pressed="false"
        onclick="toggleAutoStart()">Start with Windows</button>
      <button type="button" class="btn btn-secondary" id="ui-scale-btn" aria-pressed="false"
        onclick="toggleUiScale()">VR MODE</button>
    </div>
  </div>
</header>
<div id="main-center">
  <div id="main">
    <div id="config-wrap">
      <div id="config-column-divider" aria-hidden="true"></div>
      <div id="config-status"></div>
      <div id="config-scroll">
        <div id="device-list"></div>
      </div>
      <div class="btn-row">
        <button type="button" class="btn btn-secondary" onclick="addDevice()">+ Add Device</button>
        <div class="osc-port-row">
          <button type="button" class="btn btn-secondary" id="osc-mode-btn" onclick="toggleOscMode()">OSC: Query</button>
          <input type="text" id="osc-port-input" class="osc-port-input" inputmode="numeric"
            placeholder="9001" maxlength="5" title="UDP listen port"
            onblur="commitOscPortInput()" onkeydown="if (event.key === 'Enter') commitOscPortInput()">
        </div>
        <button type="button" class="btn btn-primary" onclick="saveConfig()">Save</button>
      </div>
    </div>
    <section id="log-section">
      <div id="log-scroll">
        <div id="log-box">
          <pre id="pat-bars" aria-live="polite"></pre>
          <pre id="log"></pre>
        </div>
      </div>
      <div id="log-bottom-spacer" aria-hidden="true"></div>
    </section>
  </div>
</div>
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
}

function toggleUiScale() {
  setUiLarge(!document.body.classList.contains('ui-large'));
}

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

function isColliderAdjustmentVisible(index) {
  return !!colliderAdjustmentVisibleByIndex[index];
}

function setColliderAdjustmentVisible(index, visible) {
  colliderAdjustmentVisibleByIndex[index] = visible;
  try {
    localStorage.setItem('colliderAdjustmentVisibleByIndex', JSON.stringify(colliderAdjustmentVisibleByIndex));
  } catch (_) {}
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
    btn.textContent = collapsed ? 'Show' : 'Hide';
    btn.setAttribute('aria-expanded', collapsed ? 'false' : 'true');
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

function syncColliderViz(index) {
  const p = colliderVizPayload(index);
  if (p) window.ipc.postMessage('collider-viz-update:' + JSON.stringify(p));
}

function openColliderViz(index) {
  const p = colliderVizPayload(index);
  if (p) window.ipc.postMessage('collider-viz-open:' + JSON.stringify(p));
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
            <span class="device-status unknown" id="device-status-${i}">—</span>
            <button type="button" class="btn btn-secondary btn-sm" onclick="pingDevice(${i}, true)">Ping</button>
          </div>
          <div class="device-card-body">
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
          </div>
          <label class="slider-field">
            <div class="slider-field-header">
              <span class="slider-field-title">Power</span>
              <span class="speed-value" id="max-speed-val-${i}">${d.max_speed}%</span>
            </div>
            <div class="speed-slider-row">
              <input type="range" min="0" max="${SPEED_SLIDER_STEPS}"
                aria-label="Power for device ${i + 1}"
                value="${Math.round(speedToSliderPos(d.max_speed) * SPEED_SLIDER_STEPS)}"
                oninput="onMaxSpeedChange(${i}, this)" onchange="saveConfig(true)">
            </div>
          </label>
          <section class="velocity-panel" aria-label="Headpat Mode for device ${i + 1}">
            <div class="velocity-panel-header">
              <div class="panel-title-row">
                <span class="velocity-panel-title">Headpat Mode</span>
                <button type="button" class="panel-info-btn" aria-expanded="false"
                  aria-controls="velocity-info-${i}"
                  aria-label="About Headpat Mode"
                  onclick="togglePanelInfo(event, 'velocity-info-${i}')">i</button>
              </div>
              <label class="velocity-switch">
                <input type="checkbox" class="velocity-toggle-input" role="switch"
                  aria-label="Enable Headpat Mode for device ${i + 1}"
                  ${d.use_velocity_control ? 'checked' : ''}
                  onchange="onVelocityControlChange(${i}, this)">
                <span class="velocity-toggle-track" aria-hidden="true">
                  <span class="velocity-toggle-thumb"></span>
                </span>
              </label>
            </div>
            <p class="panel-info-text hidden" id="velocity-info-${i}">
              Vibratrion strength follows how fast proximity changes, not how close you are.
            </p>
            <div class="velocity-panel-body${d.use_velocity_control ? '' : ' hidden'}">
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
            <div class="proximity-band-panel-header">
              <span class="proximity-band-panel-title">Collider adjustment</span>
              <div class="proximity-band-hide-row">
                <button type="button" class="btn btn-secondary btn-sm"
                  aria-label="Open collider visualization for device ${i + 1}"
                  onclick="openColliderViz(${i})">Visualize</button>
                <button type="button" class="btn btn-secondary btn-sm" id="collider-adjust-toggle-${i}"
                  aria-expanded="${isColliderAdjustmentVisible(i) ? 'true' : 'false'}"
                  aria-label="${isColliderAdjustmentVisible(i) ? 'Hide' : 'Show'} collider adjustment for device ${i + 1}"
                  onclick="toggleColliderAdjustment(${i})">${isColliderAdjustmentVisible(i) ? 'Hide' : 'Show'}</button>
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
            ${pendingRemoveIndex === i
              ? `<button type="button" class="btn btn-danger" onclick="cancelRemoveDevice()">Remove</button>
                 <button type="button" class="btn btn-secondary btn-sm" onclick="cancelRemoveDevice()">Cancel</button>
                 <button type="button" class="btn btn-primary btn-sm" onclick="confirmRemoveDevice(${i})">Confirm</button>`
              : `<button type="button" class="btn btn-danger" onclick="requestRemoveDevice(${i})">Remove</button>`}
            <button type="button" class="btn btn-secondary btn-sm device-card-toggle-btn" id="device-card-toggle-${i}"
              aria-expanded="${isDeviceCardCollapsed(i) ? 'false' : 'true'}"
              aria-label="${isDeviceCardCollapsed(i) ? 'Show' : 'Hide'} device card details for device ${i + 1}"
              onclick="toggleDeviceCardCollapse(${i})">${isDeviceCardCollapsed(i) ? 'Show' : 'Hide'}</button>
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
  const btnRow = document.querySelector('#config-wrap .btn-row');
  const spacer = document.getElementById('log-bottom-spacer');
  if (btnRow && spacer) {
    spacer.style.height = btnRow.offsetHeight + 'px';
  }

  const wrap = document.getElementById('config-wrap');
  const list = document.getElementById('device-list');
  if (!wrap || !list) return;
}

function pingStatusLabel(st) {
  if (st === 'online') return 'Online';
  if (st === 'offline') return 'Offline';
  if (st === 'checking') return 'Checking…';
  return '—';
}

function updatePingBadges() {
  editorDevices.forEach((d, i) => {
    const el = document.getElementById('device-status-' + i);
    if (!el) return;
    const ip = (d.ip || '').trim();
    const st = ip ? (devicePingStatus[ip] || 'unknown') : 'unknown';
    el.className = 'device-status ' + st;
    el.textContent = pingStatusLabel(st);
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

function onMaxSpeedChange(index, input) {
  const t = parseInt(input.value, 10) / SPEED_SLIDER_STEPS;
  const speed = sliderPosToSpeed(t);
  editorDevices[index].max_speed = speed;
  input.value = Math.round(speedToSliderPos(speed) * SPEED_SLIDER_STEPS);
  const label = document.getElementById('max-speed-val-' + index);
  if (label) label.textContent = speed + '%';
}

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
  editorDevices[index].use_velocity_control = !!input.checked;
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
    btn.textContent = visible ? 'Hide' : 'Show';
    btn.setAttribute('aria-expanded', visible ? 'true' : 'false');
    btn.setAttribute('aria-label', (visible ? 'Hide' : 'Show') + ' collider adjustment for device ' + (index + 1));
  }
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
  editorDevices = (state.devices || []).map((d) => ({
    ...d,
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

  // Startup-only: fit window height to the device-card layout (do not change width).
  requestAnimationFrame(() => {
    const header = document.querySelector('header');
    const wrap = document.getElementById('config-wrap');
    const scroll = document.getElementById('config-scroll');
    const list = document.getElementById('device-list');
    const btnRow = document.querySelector('#config-wrap .btn-row');
    if (!wrap || !scroll || !list || !btnRow) return;

    // We want enough window height to show ONE full device card + the footer row.
    // (User can manually resize taller to see more.)
    const wrapZoom = parseFloat(getComputedStyle(wrap).zoom || '1');
    const zoom = Number.isFinite(wrapZoom) && wrapZoom > 0 ? wrapZoom : 1;
    const wrapPad = parseFloat(getComputedStyle(wrap).paddingTop || '0')
      + parseFloat(getComputedStyle(wrap).paddingBottom || '0');
    const headerH = header ? header.getBoundingClientRect().height : 0;
    const statusEl = document.getElementById('config-status');
    const statusH = statusEl ? statusEl.getBoundingClientRect().height : 0;
    const gap = 12; // matches CSS gap in #config-wrap

    const firstCard = list.querySelector('.device-card');
    const listStyle = getComputedStyle(list);
    const listPadTop = parseFloat(listStyle.paddingTop || '0');
    const listPadBottom = parseFloat(listStyle.paddingBottom || '0');
    const listGap = parseFloat(listStyle.rowGap || listStyle.gap || '0') || 0;
    const cardH = firstCard ? firstCard.getBoundingClientRect().height : 0;
    const deviceOneCardH = Math.max(scroll.getBoundingClientRect().height, listPadTop + cardH + listGap + listPadBottom);

    // Small extra slack to avoid clipping by a few pixels.
    const slack = 1100;
    const configH = (wrapPad + statusH + deviceOneCardH + gap + btnRow.getBoundingClientRect().height + 24 + slack) * zoom;
    const h = Math.max(1, Math.ceil(headerH + configH));
    window.ipc.postMessage('startup-height:' + JSON.stringify({ h }));
  });
};

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
const configBtnRow = document.querySelector('#config-wrap .btn-row');
const deviceList = document.getElementById('device-list');
if (typeof ResizeObserver !== 'undefined') {
  if (configBtnRow) {
    new ResizeObserver(() => syncLogSectionLayout()).observe(configBtnRow);
  }
  if (deviceList) {
    new ResizeObserver(() => syncLogSectionLayout()).observe(deviceList);
  }
}
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
  OUTPUT_HTML.replace(LOGO_PLACEHOLDER, &uri)
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
  PingResults(String),
  MdnsLookupResult(String),
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

struct ColliderVizEntry {
  window: Window,
  webview: wry::WebView,
  state: ColliderVizState,
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
  collider_viz: HashMap<usize, ColliderVizEntry>,
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
      collider_viz: HashMap::new(),
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

  fn push_collider_viz_state(entry: &ColliderVizEntry, state: &ColliderVizState) {
    let _ = entry
      .webview
      .evaluate_script(&collider_viz::state_script(state));
  }

  fn flush_collider_live_to(entry: &ColliderVizEntry, prox_batch: &HashMap<String, f32>, headpat_batch: &HashMap<String, String>) {
    let active = &entry.state;
    let active_key = collider_viz::param_key(&active.proximity_parameter);
    if let Some(&value) = prox_batch.get(&active_key) {
      let _ = entry
        .webview
        .evaluate_script(&collider_viz::prox_sample_script(value));
    }
    if active.velocity {
      let motor_live = (!active.device_ip.is_empty())
        .then(|| PENDING_MOTOR_BARS.lock().unwrap().get(&active.device_ip).copied())
        .flatten();
      if let Some(json) = headpat_batch.get(&active_key) {
        let script = motor_live
          .map(|motor| merge_headpat_telemetry_motor(json, motor))
          .unwrap_or_else(|| json.clone());
        let _ = entry
          .webview
          .evaluate_script(&collider_viz::headpat_telemetry_script(&script));
      }
    } else if !active.device_ip.is_empty() {
      let motor = PENDING_MOTOR_BARS
        .lock()
        .unwrap()
        .get(&active.device_ip)
        .copied();
      if let Some(motor) = motor {
        if let Ok(json) = serde_json::to_string(&serde_json::json!({
          "pre": 0.0,
          "damped": 0.0,
          "smooth": 0.0,
          "motor": motor,
        })) {
          let _ = entry
            .webview
            .evaluate_script(&collider_viz::headpat_telemetry_script(&json));
        }
      }
    }
  }

  fn flush_collider_live(&mut self) {
    if self.collider_viz.is_empty() {
      return;
    }
    let prox_batch: HashMap<String, f32> = PENDING_PROX_SIGNALS.lock().unwrap().drain().collect();
    let headpat_batch: HashMap<String, String> =
      PENDING_HEADPAT_TELEMETRY.lock().unwrap().drain().collect();
    for entry in self.collider_viz.values() {
      Self::flush_collider_live_to(entry, &prox_batch, &headpat_batch);
    }
  }

  fn collider_viz_title(state: &ColliderVizState) -> String {
    let name = state.name.trim();
    if name.is_empty() {
      format!("Collider · Device {}", state.index + 1)
    } else {
      format!("Collider · {}", name)
    }
  }

  fn show_collider_viz(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    state: ColliderVizState,
  ) {
    if let Some(entry) = self.collider_viz.get_mut(&state.index) {
      entry.state = state.clone();
      entry.window.set_visible(true);
      entry.window.set_focus();
      Self::push_collider_viz_state(entry, &state);
      let _ = entry
        .webview
        .evaluate_script("requestAnimationFrame(layoutAll);");
      return;
    }

    let window = WindowBuilder::new()
      .with_title(Self::collider_viz_title(&state))
      .with_inner_size(LogicalSize::new(COLLIDER_VIZ_WIDTH, COLLIDER_VIZ_HEIGHT))
      .with_min_inner_size(LogicalSize::new(
        COLLIDER_VIZ_MIN_WIDTH,
        COLLIDER_VIZ_MIN_HEIGHT,
      ))
      .with_resizable(true)
      .with_window_icon(Some(load_tao_icon_from_ico(32)))
      .build(event_loop)
      .expect("Failed to create collider viz window");

    let webview = WebViewBuilder::with_web_context(&mut self.web_context)
      .with_html(collider_viz::COLLIDER_VIZ_HTML.to_string())
      .build(&window)
      .expect("Failed to create collider viz webview");

    let entry = ColliderVizEntry {
      window,
      webview,
      state: state.clone(),
    };
    entry.window.set_visible(true);
    entry.window.set_focus();
    Self::push_collider_viz_state(&entry, &state);
    let _ = entry
      .webview
      .evaluate_script("requestAnimationFrame(layoutAll);");
    self.collider_viz.insert(state.index, entry);
  }

  fn close_collider_viz_if_window(&mut self, window_id: tao::window::WindowId) -> bool {
    if let Some(index) = self
      .collider_viz
      .iter()
      .find(|(_, e)| e.window.id() == window_id)
      .map(|(i, _)| *i)
    {
      self.collider_viz.remove(&index);
      true
    } else {
      false
    }
  }

  fn close_all_collider_viz(&mut self) {
    self.collider_viz.clear();
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
  log_ui::set_prox_signal_notify(move |param, value| {
    queue_prox_signal(param.to_string(), value, &prox_signal_proxy);
  });

  let headpat_proxy = event_loop.create_proxy();
  log_ui::set_headpat_telemetry_notify(move |param, json| {
    queue_headpat_telemetry(param.to_string(), json.to_string(), &headpat_proxy);
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

      Event::UserEvent(UserEvent::ColliderProxFlush) => {
        ui_state.flush_collider_live();
      }

      Event::UserEvent(UserEvent::ColliderVizOpen(json)) => {
        if let Some(state) = collider_viz::parse_state(&json) {
          ui_state.show_collider_viz(event_loop, state);
        }
      }

      Event::UserEvent(UserEvent::ColliderVizUpdate(json)) => {
        if let Some(state) = collider_viz::parse_state(&json) {
          if let Some(entry) = ui_state.collider_viz.get_mut(&state.index) {
            entry.state = state.clone();
            UiState::push_collider_viz_state(entry, &state);
          }
        }
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

      Event::WindowEvent {
        window_id,
        event: WindowEvent::CloseRequested,
        ..
      } if ui_state.close_collider_viz_if_window(window_id) => {}

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
