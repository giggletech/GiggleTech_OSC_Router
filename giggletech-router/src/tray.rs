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

use serde::Deserialize;
use tao::event_loop::EventLoopProxy;

use crate::config_editor;
use crate::device_ping;
use crate::device_test;
use crate::log_ui;

const AUTO_START_VALUE_NAME: &str = "GiggleTechOSCRouter";
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const LOGO_PLACEHOLDER: &str = "{{LOGO_URI}}";
const OUTPUT_WINDOW_WIDTH: f64 = 1080.0;
const OUTPUT_WINDOW_HEIGHT: f64 = 720.0;
const OUTPUT_WINDOW_MIN_WIDTH: f64 = 960.0;
const OUTPUT_WINDOW_MIN_HEIGHT: f64 = 480.0;

static PENDING_MOTOR_BARS: Lazy<Mutex<HashMap<String, f32>>> =
  Lazy::new(|| Mutex::new(HashMap::new()));
static PENDING_PAT_BARS: Lazy<Mutex<HashMap<String, String>>> =
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

fn queue_motor_bar(param: String, value: f32, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_MOTOR_BARS.lock().unwrap().insert(param, value);
  queue_live_ui_flush(proxy);
}

fn queue_pat_bar(param: String, graph: String, proxy: &EventLoopProxy<UserEvent>) {
  PENDING_PAT_BARS.lock().unwrap().insert(param, graph);
  queue_live_ui_flush(proxy);
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
#config-wrap {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  padding: 16px 16px 0 16px;
  gap: 12px;
  overflow: hidden;
  position: relative;
}
#config-column-divider {
  position: absolute;
  right: 0;
  width: 1px;
  background: #2a2a36;
  pointer-events: none;
  display: none;
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
  padding-left: 35px;
  padding-right: 3px;
  padding-bottom: 20px;
  padding-top: 16px;
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
  margin-left: 16px;
  margin-right: 3px;
  font-size: 0.85rem;
  padding: 8px 12px;
  border-radius: 8px;
  display: none;
}
#config-status.err { display: block; background: #450a0a; color: #fca5a5; }
#device-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding-left: 16px;
  padding-right: 3px;
}
.device-card {
  width: 100%;
  /* test-slider-col 72px − horizontal padding 20px */
  --test-slider-track-width: calc(72px - 20px);
  background: #16161e;
  border: 1px solid #2a2a36;
  border-radius: 10px;
  overflow: hidden;
}
.device-card-layout {
  display: flex;
  flex-direction: row;
  align-items: stretch;
  min-height: 220px;
}
.device-main {
  flex: 1;
  min-width: 0;
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.device-card h3 { font-size: 1.8rem; color: #c4b5fd; }
.device-name-input {
  width: 100%;
  font-size: 1.9rem;
  font-weight: 600;
  font-family: inherit;
  color: #c4b5fd;
  background: transparent;
  border: none;
  border-bottom: 1px dashed #3f3f4e;
  padding: 4px 0 6px;
  margin: 0 0 4px;
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
  gap: 10px;
  flex-wrap: wrap;
}
.device-name-row .device-name-input {
  flex: 1;
  min-width: 8rem;
  margin-bottom: 0;
}
.device-status {
  flex-shrink: 0;
  font-size: 0.8rem;
  font-weight: 600;
  padding: 4px 10px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  line-height: 1;
  border-radius: 999px;
  border: 1px solid #3f3f4e;
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
  padding: 6px 12px;
  font-size: 0.8rem;
}
.device-name-row .btn-sm {
  /* Match the one-line status pill styling/size. */
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 4px 10px;
  border-radius: 999px;
  border: 1px solid #3f3f4e;
  background: #0f0f14;
  color: #b8b8c8;
  font-weight: 600;
  line-height: 1;
}
.device-card label { display: flex; flex-direction: column; gap: 4px; font-size: 0.8rem; color: #a1a1b5; }
.device-card input:not(.device-name-input) {
  padding: 8px 10px;
  border-radius: 6px;
  border: 1px solid #3f3f4e;
  background: #0f0f14;
  color: #e8e8f0;
  font-size: 0.9rem;
}
.device-card input.device-name-input {
  font-size: 1.9rem;
  padding: 6px 0 8px;
  background: transparent;
  border: none;
  border-bottom: 1px dashed #3f3f4e;
  border-radius: 0;
}
.device-card input:not([type="range"]):focus { outline: none; border-color: #a855f7; }
.device-card input.device-name-input:focus {
  border-bottom-color: #a855f7;
  border-bottom-style: solid;
}
.max-speed-block {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 4px;
}
.speed-slider-row {
  display: flex;
  align-items: center;
  gap: 16px;
  width: 100%;
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
  height: 20px;
  border-radius: 10px;
  background: #2a2a36;
  border: 1px solid #3f3f4e;
}
.speed-slider-row input[type="range"]::-webkit-slider-thumb {
  -webkit-appearance: none;
  width: var(--test-slider-track-width);
  height: var(--test-slider-track-width);
  margin-top: calc((20px - var(--test-slider-track-width)) / 2);
  border-radius: 50%;
  background: linear-gradient(135deg, #e879f9, #7c3aed);
  border: 3px solid #f3e8ff;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);
}
.speed-slider-row input[type="range"]::-moz-range-track {
  height: 20px;
  border-radius: 10px;
  background: #2a2a36;
  border: 1px solid #3f3f4e;
}
.speed-slider-row input[type="range"]::-moz-range-thumb {
  width: var(--test-slider-track-width);
  height: var(--test-slider-track-width);
  border-radius: 50%;
  background: linear-gradient(135deg, #e879f9, #7c3aed);
  border: 3px solid #f3e8ff;
  box-shadow: 0 2px 10px rgba(0, 0, 0, 0.45);
}
.speed-value {
  flex-shrink: 0;
  min-width: 56px;
  font-size: 1.15rem;
  font-weight: 600;
  color: #c4b5fd;
  text-align: right;
}
.device-fields {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.velocity-toggle-row {
  flex-direction: row !important;
  align-items: center;
  justify-content: space-between;
  gap: 12px !important;
  margin-top: 4px;
  cursor: pointer;
  user-select: none;
}
.velocity-toggle-row span {
  font-size: 0.85rem;
  color: #c4b5fd;
  font-weight: 600;
}
.velocity-toggle {
  position: relative;
  width: 44px;
  height: 24px;
  flex-shrink: 0;
  margin: 0;
  padding: 0;
  border: none;
  border-radius: 999px;
  background: #2a2a36;
  cursor: pointer;
  transition: background 0.15s ease;
  -webkit-appearance: none;
  appearance: none;
}
.velocity-toggle::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: #b8b8c8;
  transition: transform 0.15s ease, background 0.15s ease;
}
.velocity-toggle:checked {
  background: linear-gradient(135deg, #a855f7, #7c3aed);
}
.velocity-toggle:checked::after {
  transform: translateX(20px);
  background: #f3e8ff;
}
.velocity-toggle:focus { outline: none; }
.velocity-toggle:focus-visible {
  box-shadow: 0 0 0 2px #000, 0 0 0 4px #a855f7;
}
.velocity-toggle-row.velocity-sub-toggle {
  margin-left: 12px;
  padding-left: 8px;
  border-left: 2px solid #2a2a36;
}
.velocity-toggle-row.velocity-sub-toggle span {
  font-size: 0.8rem;
  color: #a1a1b5;
  font-weight: 500;
}
.velocity-toggle-row.velocity-sub-toggle.disabled {
  opacity: 0.45;
  pointer-events: none;
}
.device-card label .hint {
  display: block;
  margin-top: 2px;
  line-height: 1.35;
}
.test-slider-col {
  flex: 0 0 72px;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 12px 10px;
  border-left: 1px solid #2a2a36;
  background: #0f0f14;
  align-self: stretch;
}
.test-slider-label {
  flex-shrink: 0;
  font-size: 0.75rem;
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
  border: 2px solid #3f3f4e;
  border-radius: 10px;
  cursor: pointer;
  touch-action: none;
  user-select: none;
}
.test-slider-track.active {
  border-color: #c026d3;
  box-shadow: 0 0 12px rgba(192, 38, 211, 0.35);
}
.test-slider-arrow {
  position: absolute;
  bottom: 8px;
  left: 50%;
  z-index: 2;
  width: 14px;
  height: 14px;
  margin-left: -7px;
  pointer-events: none;
  border-top: 2.5px solid #c4b5fd;
  border-right: 2.5px solid #c4b5fd;
  transform: rotate(-45deg);
  opacity: 0.85;
  filter: drop-shadow(0 1px 2px rgba(0, 0, 0, 0.5));
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
  border-radius: 0 0 8px 8px;
  pointer-events: none;
}
.device-actions { display: flex; gap: 8px; flex-wrap: wrap; align-items: center; margin-top: 16px; }
.device-actions .btn[disabled] { opacity: 0.55; cursor: default; }
.device-actions .btn-sm {
  /* Make Confirm/Cancel match pill sizing. */
  display: inline-flex;
  align-items: center;
  justify-content: center;
  padding: 4px 10px;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 600;
  line-height: 1;
}
.device-actions .btn-secondary.btn-sm {
  border: 1px solid #3f3f4e;
  background: #2a2a36;
  color: #e8e8f0;
}
.device-actions .btn-secondary.btn-sm:hover {
  background: #3f3f4e;
}
.device-actions .btn-primary.btn-sm {
  border: 1px solid #7f1d1d;
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
  padding: 4px 10px;
  border-radius: 999px;
  font-size: 0.8rem;
  font-weight: 600;
  line-height: 1;
  border: 1px solid #3f3f4e;
  background: #2a2a36;
  color: #b8b8c8;
}
.device-actions .btn-danger:hover {
  background: #3f3f4e;
}
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
.osc-port-row {
  display: inline-flex;
  align-items: center;
  gap: 8px;
}
.btn-row .btn-primary {
  margin-left: auto;
}
#osc-mode-btn.osc-query-active {
  border: 1px solid #7c3aed;
  background: #2e1065;
  color: #e9d5ff;
}
#osc-mode-btn.osc-query-active:hover { background: #4c1d95; }
.osc-port-input {
  display: none;
  width: 5.5rem;
  flex-shrink: 0;
  padding: 9px 12px;
  font-size: 0.85rem;
  font-weight: 600;
  font-family: inherit;
  color: #e8e8f0;
  border-radius: 8px;
  border: 1px solid #3f3f4e;
  background: #0f0f14;
  box-sizing: border-box;
}
.osc-port-input:focus {
  outline: none;
  border-color: #a855f7;
}
.btn-danger { background: #450a0a; color: #fca5a5; }
.btn-danger:hover { background: #7f1d1d; }
.hint { font-size: 0.8rem; color: #6b6b80; margin-top: 4px; }
</style>
</head>
<body>
<header>
  <div class="header-inner">
    <div class="header-config-col">
      <img class="header-logo" src="{{LOGO_URI}}" alt="GiggleTech">
    </div>
    <div class="header-log-col" aria-hidden="true"></div>
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
let editorDevices = [];
let editorSpeedDefaults = { min: 5, max: 25 };
let editorVelocityDefault = false;
let editorVelocityOnProxDropDefault = false;
let editorPortRx = 'OSCQuery';
let devicePingStatus = {};
let pingDebounceTimer = null;
const PING_POLL_MS = 5000;
let pingPollTimer = null;
let pingInFlight = false;
let pendingRemoveIndex = null;
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

function editorValidationOk() {
  if (!editorDevices.length) return false;
  for (const d of editorDevices) {
    if (!d.ip.trim() || !isValidIp(d.ip)) return false;
    if (!(d.proximity_parameter || '').trim()) return false;
    if (d.max_speed < editorSpeedDefaults.min || d.max_speed > 100) return false;
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
    <div class="device-card">
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
          <div class="device-fields">
            <label>IP address
              <input type="text" value="${escapeHtml(d.ip)}"
                oninput="editorDevices[${i}].ip=this.value; onDeviceIpChange(${i}); maybeClearConfigError()">
            </label>
            <label>Proximity parameter
              <input type="text" value="${escapeHtml(d.proximity_parameter)}" placeholder="proximity_01"
                oninput="editorDevices[${i}].proximity_parameter=this.value; maybeClearConfigError()">
            </label>
          </div>
          <label class="velocity-toggle-row">
            <span>Velocity control</span>
            <input type="checkbox" class="velocity-toggle" role="switch"
              aria-label="Velocity control for device ${i + 1}"
              ${d.use_velocity_control ? 'checked' : ''}
              onchange="onVelocityControlChange(${i}, this)">
          </label>
          <label class="velocity-toggle-row velocity-sub-toggle${d.use_velocity_control ? '' : ' disabled'}">
            <span>Vibrate on pull-away</span>
            <input type="checkbox" class="velocity-toggle" role="switch"
              id="velocity-on-drop-${i}"
              aria-label="Vibrate on proximity drop for device ${i + 1}"
              ${d.velocity_on_prox_drop ? 'checked' : ''}
              ${d.use_velocity_control ? '' : 'disabled'}
              onchange="onVelocityOnProxDropChange(${i}, this)">
          </label>
          <label class="max-speed-block">Power
            <div class="speed-slider-row">
              <input type="range" min="0" max="${SPEED_SLIDER_STEPS}"
                value="${Math.round(speedToSliderPos(d.max_speed) * SPEED_SLIDER_STEPS)}"
                oninput="onMaxSpeedChange(${i}, this)" onchange="saveConfig(true)">
              <span class="speed-value" id="max-speed-val-${i}">${d.max_speed}%</span>
            </div>
          </label>
          <div class="device-actions">
            ${pendingRemoveIndex === i
              ? `<button type="button" class="btn btn-danger" onclick="cancelRemoveDevice()">Remove</button>
                 <button type="button" class="btn btn-secondary btn-sm" onclick="cancelRemoveDevice()">Cancel</button>
                 <button type="button" class="btn btn-primary btn-sm" onclick="confirmRemoveDevice(${i})">Confirm</button>`
              : `<button type="button" class="btn btn-danger" onclick="requestRemoveDevice(${i})">Remove</button>`}
          </div>
        </div>
        <div class="test-slider-col">
          <span class="test-slider-label">Motor</span>
          <div class="test-slider-track" data-index="${i}">
            <span class="test-slider-arrow" aria-hidden="true"></span>
            <div class="test-slider-fill"></div>
          </div>
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
  const divider = document.getElementById('config-column-divider');
  if (!wrap || !list || !divider) return;

  const hasCards = list.querySelector('.device-card');
  if (!hasCards) {
    divider.style.display = 'none';
    return;
  }

  const wrapRect = wrap.getBoundingClientRect();
  const listRect = list.getBoundingClientRect();
  divider.style.display = 'block';
  divider.style.top = (listRect.top - wrapRect.top) + 'px';
  divider.style.height = listRect.height + 'px';
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
    if (r.ip) devicePingStatus[r.ip] = r.online ? 'online' : 'offline';
  });
  updatePingBadges();
};

function sliderValueFromEvent(trackEl, ev) {
  const rect = trackEl.getBoundingClientRect();
  const y = (ev.clientY ?? (ev.touches && ev.touches[0] && ev.touches[0].clientY) ?? rect.bottom) - rect.top;
  return 1 - Math.max(0, Math.min(1, y / rect.height));
}

function setSliderVisual(trackEl, value) {
  const fill = trackEl.querySelector('.test-slider-fill');
  if (fill) fill.style.height = Math.round(value * 100) + '%';
  trackEl.classList.toggle('active', value > 0);
  const arrow = trackEl.querySelector('.test-slider-arrow');
  if (arrow) {
    const trackH = trackEl.clientHeight || 1;
    const arrowZonePx = 24;
    arrow.classList.toggle('hidden', value > 0 && value * trackH >= arrowZonePx);
  }
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
  if (drag.trackEl) setSliderVisual(drag.trackEl, 0);
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
    setSliderVisual(trackEl, value);
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

function onVelocityControlChange(index, input) {
  if (!editorDevices[index]) return;
  editorDevices[index].use_velocity_control = !!input.checked;
  renderDevices();
  saveConfig(true);
}

function onVelocityOnProxDropChange(index, input) {
  if (!editorDevices[index]) return;
  editorDevices[index].velocity_on_prox_drop = !!input.checked;
  saveConfig(true);
}

function addDevice() {
  editorDevices.push({
    name: editorDevices.length === 0 ? 'Headpats' : '',
    ip: '',
    proximity_parameter: 'proximity_01',
    max_speed: editorSpeedDefaults.max,
    use_velocity_control: editorVelocityDefault,
    velocity_on_prox_drop: editorVelocityOnProxDropDefault
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
  if (state.port_rx != null) editorPortRx = String(state.port_rx);
  const powerMin = editorSpeedDefaults.min;
  editorDevices = (state.devices || []).map((d) => ({
    ...d,
    max_speed: Math.max(powerMin, d.max_speed ?? powerMin),
    use_velocity_control: !!d.use_velocity_control,
    velocity_on_prox_drop: !!d.velocity_on_prox_drop,
  }));
  renderDevices();
  updateOscPortUi();
  clearConfigStatus();
  startDevicePingLoop();
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

function applyMotorBars(updates) {
  if (activeTestDrag) return;
  for (const param in updates) {
    const value = Math.max(0, Math.min(1, updates[param]));
    const paramKey = (param || '').trim();
    editorDevices.forEach((d, i) => {
      if ((d.proximity_parameter || '').trim() !== paramKey) return;
      const track = document.querySelector('.test-slider-track[data-index="' + i + '"]');
      if (track) setSliderVisual(track, value);
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
  const tail = lines.slice(-statusViewportLines());
  el.textContent = tail.join('\n');
}

function setStatusLines(lines) {
  lastStatusLines = lines.slice(-STATUS_MAX_LINES);
  renderStatus(lastStatusLines);
}

function appendStatusLines(lines) {
  if (!lines || !lines.length) return;
  lastStatusLines = lastStatusLines.concat(lines);
  if (lastStatusLines.length > STATUS_MAX_LINES) {
    lastStatusLines = lastStatusLines.slice(-STATUS_MAX_LINES);
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
  ConfigIpc(String),
  PingResults(String),
}

#[derive(Debug, Deserialize)]
struct PingDevicesRequest {
  ips: Vec<String>,
}

struct OutputWindow {
  window: Window,
  webview: wry::WebView,
}

const STATUS_FLUSH_INTERVAL: Duration = Duration::from_millis(50);

struct UiState {
  output: Option<OutputWindow>,
  status_synced: usize,
  status_epoch: usize,
  status_pending: bool,
  last_status_flush: Option<Instant>,
}

impl UiState {
  fn new() -> Self {
    Self {
      output: None,
      status_synced: 0,
      status_epoch: log_ui::buffer_epoch(),
      status_pending: false,
      last_status_flush: None,
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
      .with_inner_size(LogicalSize::new(OUTPUT_WINDOW_WIDTH, OUTPUT_WINDOW_HEIGHT))
      .with_min_inner_size(LogicalSize::new(
        OUTPUT_WINDOW_MIN_WIDTH,
        OUTPUT_WINDOW_MIN_HEIGHT,
      ))
      .build(event_loop)
      .expect("Failed to create output window");

    let webview = WebViewBuilder::new()
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
    if self.output.is_none() {
      self.create_output_window(event_loop, ipc_proxy.clone());
    }

    if let Some(output) = &self.output {
      output.window.set_visible(true);
      output.window.set_focus();
      sync_status_to_webview(&output.webview, &mut self.status_synced, &mut self.status_epoch);
      self.status_pending = false;
      self.last_status_flush = Some(Instant::now());
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
    let proxy = event_proxy.clone();
    std::thread::spawn(move || {
      let results = async_std::task::block_on(device_ping::ping_hosts(&req.ips));
      let payload = match serde_json::to_string(&serde_json::json!({ "results": results })) {
        Ok(p) => p,
        Err(_) => return,
      };
      let _ = proxy.send_event(UserEvent::PingResults(payload));
    });
  }
}

/// Run the tray icon event loop on the main thread. Blocks until the user exits.
pub fn run() {
  let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
  let event_proxy = event_loop.create_proxy();
  let ipc_proxy = event_loop.create_proxy();

  log_ui::set_status_notify(move || {
    let _ = event_proxy.send_event(UserEvent::StatusUpdated);
  });

  let prox_proxy = event_loop.create_proxy();
  log_ui::set_proximity_notify(move |param, value| {
    queue_motor_bar(param.to_string(), value, &prox_proxy);
  });

  let pat_proxy = event_loop.create_proxy();
  log_ui::set_pat_bar_notify(move |param, graph| {
    queue_pat_bar(param.to_string(), graph.to_string(), &pat_proxy);
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

      Event::UserEvent(UserEvent::StatusUpdated) => {
        ui_state.on_status_updated();
      }

      Event::MainEventsCleared => {
        ui_state.flush_pending_status();
      }

      Event::UserEvent(UserEvent::LiveUiFlush) => {
        if ui_state.is_output_visible() {
          if let Some(output) = &ui_state.output {
            flush_live_ui(&output.webview);
          }
        }
      }

      Event::UserEvent(UserEvent::ConfigIpc(msg)) => {
        if let Some(output) = &ui_state.output {
          handle_config_ipc(&output.webview, &ipc_proxy, &msg);
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
