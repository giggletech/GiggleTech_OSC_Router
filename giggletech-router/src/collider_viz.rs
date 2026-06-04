//! Embedded visualizer cards: proximity rings and live OSC samples (multi-instance).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColliderVizState {
  pub index: usize,
  pub name: String,
  #[serde(default)]
  pub device_ip: String,
  pub proximity_parameter: String,
  pub outer: f32,
  pub inner: f32,
  pub velocity: bool,
  #[serde(default)]
  pub velocity_scalar: u32,
  #[serde(default = "default_velocity_softcap")]
  pub velocity_softcap: u32,
  #[serde(default)]
  pub velocity_smoothing_ms: u32,
  #[serde(default)]
  pub velocity_on_prox_drop: bool,
}

fn default_velocity_softcap() -> u32 {
  35
}

pub fn parse_state(json: &str) -> Option<ColliderVizState> {
  serde_json::from_str(json).ok()
}

/// Normalize VRChat parameter names for matching editor vs OSC paths.
pub fn param_key(s: &str) -> String {
  s.trim()
    .trim_start_matches("/avatar/parameters/")
    .to_string()
}

/// Batch key for prox/telemetry maps (device IP + parameter avoids cross-device bleed).
pub fn batch_key(device_ip: &str, proximity_parameter: &str) -> String {
  let ip = device_ip.trim();
  let param = param_key(proximity_parameter);
  if ip.is_empty() {
    param
  } else {
    format!("{ip}::{param}")
  }
}

pub fn state_script(state: &ColliderVizState) -> String {
  let json = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
  format!("window.colliderVizApi&&window.colliderVizApi.applyState({json});")
}

pub fn prox_sample_script(index: usize, value: f32) -> String {
  format!("window.colliderVizApi&&window.colliderVizApi.applyProxSample({index},{value});")
}

pub fn headpat_telemetry_script(index: usize, json: &str, append: bool) -> String {
  format!(
    "window.colliderVizApi&&window.colliderVizApi.applyHeadpatTelemetry({index},{json},{{append:{append}}});"
  )
}

pub fn headpat_motor_script(index: usize, motor: f32) -> String {
  format!("window.colliderVizApi&&window.colliderVizApi.applyMotorSample({index},{motor});")
}

/// Ring + chart in one WebView call when both samples arrive on the same flush.
pub fn collider_flush_script(
  index: usize,
  prox: Option<f32>,
  telemetry_json: Option<&str>,
  append: bool,
) -> String {
  match (prox, telemetry_json) {
    (Some(p), Some(json)) => format!(
      "window.colliderVizApi&&window.colliderVizApi.applyFlush({index},{p},{json},{{append:{append}}});"
    ),
    (Some(p), None) => prox_sample_script(index, p),
    (None, Some(json)) => headpat_telemetry_script(index, json, append),
    (None, None) => String::new(),
  }
}

/// Styles for visualizer markup inside a log-column card.
pub const COLLIDER_VIZ_STYLES: &str = r#"
.collider-viz-root {
  width: 100%;
  min-height: 0;
  font-family: "Segoe UI", system-ui, sans-serif;
  color: #e8e8f0;
}
.collider-viz-root * { box-sizing: border-box; }
.collider-viz-root .cv-app {
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 10px 12px 12px;
  gap: 6px;
}
.collider-viz-root .cv-app-scroll {
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}
.collider-viz-root .cv-viz-column {
  width: 100%;
  display: grid;
  grid-template-rows: minmax(140px, auto) minmax(110px, auto);
  gap: 10px;
}
.collider-viz-root .cv-title {
  font-size: 0.95rem;
  font-weight: 600;
  margin: 0;
}
.collider-viz-root .panel {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
  min-height: 0;
}
.collider-viz-root .cv-ring-panel {
  overflow: hidden;
}
.collider-viz-root .cv-chart-panel {
  overflow: hidden;
  padding-top: 8px;
  border-top: 1px solid #2a2a36;
}
.collider-viz-root .section-title {
  font-size: 0.75rem;
  font-weight: 600;
  color: #c8c8d8;
}
.collider-viz-root .cv-diagram-wrap {
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
.collider-viz-root .cv-ring {
  display: block;
  flex-shrink: 0;
  aspect-ratio: 1;
  cursor: crosshair;
  touch-action: none;
  background: #12121a;
  border-radius: 50%;
}
.collider-viz-root .cv-chart-legend {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 10px;
  font-size: 0.65rem;
  color: #6b6b80;
}
.collider-viz-root .cv-chart-legend span {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.collider-viz-root .swatch {
  width: 12px;
  height: 2px;
  border-radius: 999px;
}
.collider-viz-root .swatch.raw { background: #5b8def; }
.collider-viz-root .swatch.smooth { background: #ffb020; }
.collider-viz-root .swatch.motor { background: #e8e8f0; }
.collider-viz-root .chart-wrap {
  width: 100%;
  aspect-ratio: 5 / 3;
  padding: 6px 8px 8px;
}
.collider-viz-root .chart-wrap canvas {
  display: block;
  width: 100%;
  height: 100%;
  background: #12121a;
  border: 1px solid #2a2a36;
  border-radius: 8px;
}
"#;

/// Markup mounted inside each visualizer log card (no device index in DOM — scoped by root).
pub const COLLIDER_VIZ_CARD_INNER: &str = r#"<div class="collider-viz-root">
<div class="cv-app">
<div class="cv-app-scroll">
<div class="cv-viz-column">
<section class="panel cv-ring-panel">
  <div class="viz-plot-block cv-ring-plot-block">
    <div class="panel-head"><h2 class="section-title">Touch position</h2></div>
    <div class="cv-diagram-wrap">
      <canvas class="cv-ring" aria-label="Proximity rings"></canvas>
    </div>
  </div>
</section>
<section class="panel cv-chart-panel" aria-label="Output chart">
  <div class="viz-plot-block">
    <div class="panel-head"><h2 class="section-title cv-chart-title">Motor output</h2></div>
    <div class="cv-chart-legend"></div>
    <div class="chart-wrap">
      <canvas class="cv-output-chart" aria-label="Output over time"></canvas>
    </div>
  </div>
</section>
</div>
</div>
</div>
</div>"#;

pub const COLLIDER_VIZ_RUNTIME: &str = include_str!("collider_viz_runtime.js");
