//! Standalone window: proximity rings (outer / inner) and live OSC sample.

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

pub fn state_script(state: &ColliderVizState) -> String {
  let json = serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string());
  format!("window.applyColliderVizState({});", json)
}

pub fn prox_sample_script(value: f32) -> String {
  format!("window.applyColliderProxSample({});", value)
}

pub fn headpat_telemetry_script(json: &str) -> String {
  format!("window.applyHeadpatTelemetry({});", json)
}

pub const COLLIDER_VIZ_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Collider band</title>
<style>
* { box-sizing: border-box; margin: 0; padding: 0; }
html, body {
  height: 100%;
  width: 100%;
  overflow: hidden;
  font-family: "Segoe UI", system-ui, sans-serif;
  background: #0a0a0f;
  color: #e8e8f0;
}
.app {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  padding: clamp(8px, 1.2vmin, 12px) clamp(12px, 2vmin, 18px) clamp(14px, 2.5vmin, 22px);
  gap: clamp(4px, 1vmin, 6px);
}
.app-scroll {
  flex: 1 1 auto;
  min-height: 0;
  overflow-x: hidden;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  align-items: stretch;
}
.viz-column {
  flex: 1 1 0;
  width: 100%;
  max-width: 100%;
  min-width: 0;
  min-height: 0;
  display: grid;
  grid-template-rows: minmax(0, 1fr) minmax(0, 1fr);
  grid-template-columns: minmax(0, 1fr);
  gap: clamp(10px, 2vmin, 16px);
}
.header {
  flex: 0 0 auto;
  width: 100%;
  padding: clamp(2px, 0.5vmin, 4px) 0 0;
}
h1 {
  font-size: clamp(0.95rem, 2.8vmin, 1.15rem);
  font-weight: 600;
  text-align: left;
  width: 100%;
}
.viz-plot-block {
  width: 100%;
  min-height: 0;
  max-height: 100%;
  display: flex;
  flex-direction: column;
  align-items: stretch;
  gap: clamp(4px, 1vmin, 6px);
}
.ring-panel .viz-plot-block {
  flex: 1 1 0;
  min-height: 0;
  overflow: hidden;
}
.panel {
  display: flex;
  flex-direction: column;
  gap: clamp(6px, 1vmin, 8px);
  min-width: 0;
  min-height: 0;
}
.ring-panel,
.chart-panel {
  min-width: 0;
  min-height: 0;
}
.ring-panel {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  justify-content: flex-start;
  overflow: hidden;
}
.chart-panel {
  display: flex;
  flex-direction: column;
  align-items: stretch;
  justify-content: flex-start;
  overflow: hidden;
  padding: clamp(8px, 1.4vmin, 12px) clamp(4px, 0.8vmin, 8px) clamp(6px, 1.2vmin, 10px);
  border-top: 1px solid #2a2a36;
}
.panel-head {
  display: flex;
  flex-direction: column;
  gap: 2px;
  width: 100%;
  padding: 0;
}
.section-title {
  font-size: clamp(0.72rem, 2.2vmin, 0.82rem);
  font-weight: 600;
  color: #c8c8d8;
  text-align: left;
  width: 100%;
}
.diagram-wrap {
  flex: 1 1 0;
  min-width: 0;
  min-height: 0;
  max-height: 100%;
  width: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
}
#ring {
  display: block;
  flex-shrink: 0;
  aspect-ratio: 1;
  cursor: crosshair;
  touch-action: none;
  background: #12121a;
  border-radius: 50%;
}
.pipeline-only { display: block; }
body:not(.velocity-mode) .pipeline-only { display: none !important; }
.chart-legend {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-start;
  gap: clamp(4px, 1vmin, 6px) clamp(8px, 2vmin, 14px);
  font-size: clamp(0.62rem, 2vmin, 0.68rem);
  color: #6b6b80;
  width: 100%;
}
.chart-legend span {
  display: inline-flex;
  align-items: center;
  gap: clamp(4px, 1vmin, 6px);
}
.swatch {
  width: clamp(10px, 3vmin, 14px);
  height: clamp(2px, 0.6vmin, 3px);
  border-radius: 999px;
  flex-shrink: 0;
}
.swatch.raw { background: #5b8def; }
.swatch.smooth { background: #ffb020; }
.swatch.motor { background: #e8e8f0; }
.chart-panel .viz-plot-block {
  flex: 1 1 0;
  min-height: 0;
  overflow: hidden;
}
.chart-wrap {
  flex: 0 0 auto;
  width: 100%;
  min-width: 0;
  max-width: 100%;
  max-height: 100%;
  height: auto;
  min-height: 0;
  aspect-ratio: 5 / 3;
  padding: clamp(6px, 1.2vmin, 10px) clamp(8px, 1.6vmin, 14px) clamp(10px, 2vmin, 16px);
}
.chart-wrap canvas {
  display: block;
  width: 100%;
  height: 100%;
  background: #12121a;
  border: 1px solid #2a2a36;
  border-radius: clamp(6px, 1.2vmin, 10px);
}
</style>
</head>
<body>
<div class="app">
<div class="header">
  <h1 id="title">Collider</h1>
</div>
<div class="app-scroll">
<div class="viz-column">
<section class="panel ring-panel">
  <div class="viz-plot-block" id="ring-plot-block">
    <div class="panel-head">
      <h2 class="section-title">Touch position</h2>
    </div>
    <div class="diagram-wrap" id="diagram-wrap">
      <canvas id="ring" aria-label="Proximity rings"></canvas>
    </div>
  </div>
</section>
<section class="panel chart-panel" id="headpat-panel" aria-label="Output chart">
  <div class="viz-plot-block" id="chart-plot-block">
    <div class="panel-head">
      <h2 class="section-title" id="chart-section-title">Motor output</h2>
    </div>
    <div class="chart-legend" id="chart-legend"></div>
    <div class="chart-wrap">
      <canvas id="output-chart" aria-label="Output over time (0–100%)"></canvas>
    </div>
  </div>
</section>
</div>
</div>
<script>
let state = {
  index: 0,
  name: 'Device',
  device_ip: '',
  proximity_parameter: 'proximity_01',
  outer: 0,
  inner: 1,
  velocity: false,
  velocity_scalar: 20,
  velocity_softcap: 35,
  velocity_smoothing_ms: 80,
  velocity_on_prox_drop: false
};
const velHistory = { pre: [], smooth: [], motor: [] };
const VEL_HISTORY_LEN = 100;
const CHART_SCALE_MAX = 100;
const outputChart = document.getElementById('output-chart');
const outputCtx = outputChart ? outputChart.getContext('2d') : null;
const diagramWrap = document.getElementById('diagram-wrap');
let liveProx = null;
let manualProx = null;
const TRAIL_MAX = 48;
const trail = [];
const TRAIL_ANGLE = -Math.PI / 2;
const canvas = document.getElementById('ring');
const ctx = canvas.getContext('2d');
let animId = 0;

function displayProx() {
  if (manualProx != null) return manualProx;
  if (liveProx != null) return liveProx;
  return 0;
}

function proxInBand(p, s) {
  if (p < s.outer) return false;
  if (s.velocity) return p <= s.inner;
  return true;
}

function pushTrail(p) {
  const t = performance.now();
  trail.push({ p: p, t: t });
  while (trail.length > TRAIL_MAX) trail.shift();
}

/** Far (0) on the outside; close (1) toward the center. */
function proxToRadius(p, maxR) {
  return (1 - Math.max(0, Math.min(1, p))) * maxR;
}

function snap(v) {
  return Math.round(v * 2) / 2;
}

function fillAnnulus(cx, cy, rOuter, rInner, color) {
  if (rOuter <= rInner + 0.5) return;
  ctx.beginPath();
  ctx.arc(cx, cy, snap(rOuter), 0, Math.PI * 2, false);
  ctx.arc(cx, cy, snap(rInner), 0, Math.PI * 2, true);
  ctx.fillStyle = color;
  ctx.fill('evenodd');
}

function strokeCircle(cx, cy, r, color, lineW) {
  if (r < 1) return;
  ctx.save();
  ctx.strokeStyle = color;
  ctx.lineWidth = lineW;
  ctx.lineCap = 'round';
  ctx.beginPath();
  ctx.arc(cx, cy, snap(r), 0, Math.PI * 2);
  ctx.stroke();
  ctx.restore();
}

function fillCircle(cx, cy, r, color) {
  if (r < 0.5) return;
  ctx.beginPath();
  ctx.arc(cx, cy, snap(r), 0, Math.PI * 2);
  ctx.fillStyle = color;
  ctx.fill();
}

function pointOnRing(cx, cy, r, angle) {
  return {
    x: snap(cx + Math.cos(angle) * r),
    y: snap(cy + Math.sin(angle) * r)
  };
}

function sizeRingCanvas() {
  if (!diagramWrap || !canvas) return 0;
  const rect = diagramWrap.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const cssSide = Math.floor(Math.min(rect.width, rect.height));
  if (cssSide < 24) return 0;
  const side = Math.floor(cssSide * dpr);
  canvas.style.width = cssSide + 'px';
  canvas.style.height = cssSide + 'px';
  if (canvas.width !== side || canvas.height !== side) {
    canvas.width = side;
    canvas.height = side;
  }
  return side;
}

function redraw() {
  const size = sizeRingCanvas();
  if (!size || !ctx) return;
  const dpr = window.devicePixelRatio || 1;
  const w = size;
  const h = size;
  const cx = snap(w / 2);
  const cy = snap(h / 2);
  const maxR = snap(w * 0.38);
  const lineW = Math.max(2, 2 * dpr);

  const rOuter = snap(proxToRadius(state.outer, maxR));
  const rInner = snap(proxToRadius(state.inner, maxR));
  const rMax = maxR;
  const rMaxCenter = snap(10 * dpr);
  const p = Math.max(0, Math.min(1, displayProx()));
  const rSample = snap(proxToRadius(p, maxR));

  ctx.setTransform(1, 0, 0, 1, 0, 0);
  ctx.imageSmoothingEnabled = true;
  ctx.clearRect(0, 0, w, h);

  // Zone fills (clean annuli + center disk)
  fillAnnulus(cx, cy, rMax, rOuter, '#1a1a24');
  fillAnnulus(cx, cy, rOuter, rInner, 'rgba(45, 106, 79, 0.4)');
  fillCircle(cx, cy, rInner, 'rgba(92, 74, 26, 0.38)');

  // Trail dots on the top radial (clean small circles)
  const now = performance.now();
  for (let i = 0; i < trail.length; i++) {
    const pt = trail[i];
    const age = (now - pt.t) / 1400;
    const alpha = Math.max(0.06, 1 - age);
    const r = snap(proxToRadius(pt.p, maxR));
    const ptXY = pointOnRing(cx, cy, r, TRAIL_ANGLE);
    fillCircle(ptXY.x, ptXY.y, snap(3 * dpr + alpha * 2 * dpr), 'rgba(110, 231, 168, ' + (alpha * 0.5) + ')');
  }

  // Concentric ring strokes (outer → inner)
  strokeCircle(cx, cy, rOuter, '#4a7cff', lineW);
  if (rInner >= rMaxCenter + 2) strokeCircle(cx, cy, rInner, '#ff9f43', lineW);

  // Live sample — clean circle on the ring line
  const inBand = proxInBand(p, state);
  const atZero = p <= 0.001;
  const sample = pointOnRing(cx, cy, rSample, TRAIL_ANGLE);
  const dotR = snap(5 * dpr);
  const dotFill = atZero ? '#e85d5d' : (inBand ? '#6ee7a8' : '#5a5a6e');
  const dotStroke = atZero ? '#f0a0a0' : (inBand ? '#e8e8f0' : '#f0a0a0');
  fillCircle(sample.x, sample.y, dotR, dotFill);
  strokeCircle(sample.x, sample.y, dotR, dotStroke, lineW * 0.85);
}

function toChartPct(v) {
  return Math.min(CHART_SCALE_MAX, Math.max(0, v));
}

function motorToChartPct(motor) {
  return Math.min(CHART_SCALE_MAX, Math.max(0, motor * 100));
}

function motorHistoryPct() {
  const arr = velHistory.motor;
  const out = new Array(arr.length);
  for (let i = 0; i < arr.length; i++) out[i] = motorToChartPct(arr[i]);
  return out;
}

function updateChartLegend() {
  const legend = document.getElementById('chart-legend');
  if (!legend) return;
  if (state.velocity) {
    let html = '<span><i class="swatch raw"></i> Touch raw</span>';
    html += '<span><i class="swatch smooth"></i> Touch smoothed</span>';
    html += '<span><i class="swatch motor"></i> Motor</span>';
    legend.innerHTML = html;
  } else {
    legend.innerHTML = '<span><i class="swatch motor"></i> Motor</span>';
  }
}

function pushVelHistory(key, v) {
  const arr = velHistory[key];
  arr.push(Math.max(0, v));
  while (arr.length > VEL_HISTORY_LEN) arr.shift();
}

function sizeChartCanvas(chart) {
  if (!chart) return null;
  const rect = chart.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  const cssW = rect.width;
  const cssH = rect.height;
  if (cssW < 8 || cssH < 8) return null;
  const w = Math.floor(cssW * dpr);
  const h = Math.floor(cssH * dpr);
  if (chart.width !== w || chart.height !== h) {
    chart.width = w;
    chart.height = h;
  }
  return { w: w, h: h, dpr: dpr };
}

function drawSeriesOn(ctx, arr, color, maxV, w, h, dpr, plot) {
  if (!ctx || arr.length < 2) return;
  const scale = Math.max(0.001, maxV);
  const plotW = w - plot.padL - plot.padR;
  const plotH = h - plot.padT - plot.padB;
  ctx.strokeStyle = color;
  ctx.lineWidth = 2 * dpr;
  ctx.lineJoin = 'round';
  ctx.beginPath();
  const step = plotW / Math.max(1, VEL_HISTORY_LEN - 1);
  const x0 = plot.padL + Math.max(0, VEL_HISTORY_LEN - arr.length) * step;
  for (let i = 0; i < arr.length; i++) {
    const x = x0 + i * step;
    const y = h - plot.padB - (arr[i] / scale) * plotH;
    if (i === 0) ctx.moveTo(x, y);
    else ctx.lineTo(x, y);
  }
  ctx.stroke();
}

function chartPlotMetrics(w, h, dpr) {
  return {
    maxV: CHART_SCALE_MAX,
    padL: 40 * dpr,
    padR: 20 * dpr,
    padT: 16 * dpr,
    padB: 26 * dpr
  };
}

function drawHLine(ctx, x0, x1, y, color, dpr, dash) {
  ctx.strokeStyle = color;
  ctx.lineWidth = 1 * dpr;
  ctx.setLineDash(dash ? [4 * dpr, 4 * dpr] : []);
  ctx.beginPath();
  ctx.moveTo(x0, y);
  ctx.lineTo(x1, y);
  ctx.stroke();
  ctx.setLineDash([]);
}

function drawChartAxes(ctx, w, h, dpr) {
  const plot = chartPlotMetrics(w, h, dpr);
  const plotH = h - plot.padT - plot.padB;
  const x0 = plot.padL;
  const x1 = w - plot.padR;
  const labelStyle = 'rgba(107, 107, 128, 0.85)';
  const gridStyle = 'rgba(107, 107, 128, 0.22)';
  ctx.font = (8 * dpr) + 'px Segoe UI, system-ui, sans-serif';
  ctx.fillStyle = labelStyle;
  ctx.textAlign = 'right';
  ctx.textBaseline = 'middle';
  const y100 = plot.padT;
  const y50 = plot.padT + plotH * 0.5;
  const y0 = h - plot.padB;
  drawHLine(ctx, x0, x1, y100, gridStyle, dpr, true);
  drawHLine(ctx, x0, x1, y50, gridStyle, dpr, true);
  drawHLine(ctx, x0, x1, y0, gridStyle, dpr, false);
  ctx.fillText('100', plot.padL - 8 * dpr, y100);
  ctx.fillText('50', plot.padL - 8 * dpr, y50);
  ctx.fillText('0', plot.padL - 8 * dpr, y0);
  return plot;
}

function drawOutputChart() {
  if (!outputCtx) return;
  const dims = sizeChartCanvas(outputChart);
  if (!dims) return;
  const w = dims.w;
  const h = dims.h;
  const dpr = dims.dpr;
  outputCtx.clearRect(0, 0, w, h);
  const plot = drawChartAxes(outputCtx, w, h, dpr);
  const maxV = plot.maxV;
  if (state.velocity) {
    drawSeriesOn(outputCtx, velHistory.pre, '#5b8def', maxV, w, h, dpr, plot);
    drawSeriesOn(outputCtx, velHistory.smooth, '#ffb020', maxV, w, h, dpr, plot);
    drawSeriesOn(outputCtx, motorHistoryPct(), '#e8e8f0', maxV, w, h, dpr, plot);
  } else {
    drawSeriesOn(outputCtx, motorHistoryPct(), '#e8e8f0', maxV, w, h, dpr, plot);
  }
}

function updateHeadpatPanel() {
  document.body.classList.toggle('velocity-mode', !!state.velocity);
  const chartTitle = document.getElementById('chart-section-title');
  if (chartTitle) {
    chartTitle.textContent = state.velocity ? 'Touch → motor' : 'Motor output';
  }
  updateChartLegend();
  requestAnimationFrame(layoutAll);
}

function clearVelHistory() {
  velHistory.pre.length = 0;
  velHistory.smooth.length = 0;
  velHistory.motor.length = 0;
  drawOutputChart();
}

function updateModeText() {
  document.getElementById('title').textContent = 'Collider · ' + (state.name || 'Device');
}

window.applyColliderVizState = function(s) {
  const prevIndex = state.index;
  state = Object.assign({}, state, s);
  if (state.inner <= state.outer) {
    state.inner = Math.min(1, state.outer + 0.01);
  }
  if (s.index != null && s.index !== prevIndex) clearVelHistory();
  updateModeText();
  updateHeadpatPanel();
  layoutAll();
};

window.applyHeadpatTelemetry = function(t) {
  const sample = {
    pre: Number(t.pre) || 0,
    smooth: Number(t.smooth) || 0,
    motor: Math.max(0, Math.min(1, Number(t.motor) || 0))
  };
  pushVelHistory('motor', sample.motor);
  if (state.velocity) {
    pushVelHistory('pre', toChartPct(sample.pre));
    pushVelHistory('smooth', toChartPct(sample.smooth));
  }
  drawOutputChart();
};

window.applyColliderProxSample = function(v) {
  const p = Math.max(0, Math.min(1, Number(v) || 0));
  liveProx = p;
  if (manualProx == null) {
    if (trail.length === 0 || Math.abs(trail[trail.length - 1].p - p) > 0.002) {
      pushTrail(p);
    }
  }
  redraw();
};

function pointerToProx(clientX, clientY) {
  const rect = canvas.getBoundingClientRect();
  if (!rect.width || !rect.height) return 0;
  const dpr = canvas.width / rect.width;
  const cx = canvas.width / 2;
  const cy = canvas.height / 2;
  const maxR = canvas.width * 0.38;
  const px = (clientX - rect.left) * dpr;
  const py = (clientY - rect.top) * dpr;
  const dx = px - cx;
  const dy = py - cy;
  const dist = Math.sqrt(dx * dx + dy * dy);
  return Math.max(0, Math.min(1, 1 - dist / maxR));
}

canvas.addEventListener('pointerdown', (e) => {
  manualProx = pointerToProx(e.clientX, e.clientY);
  pushTrail(manualProx);
  canvas.setPointerCapture(e.pointerId);
  redraw();
});
canvas.addEventListener('pointermove', (e) => {
  if (!canvas.hasPointerCapture(e.pointerId)) return;
  manualProx = pointerToProx(e.clientX, e.clientY);
  pushTrail(manualProx);
  redraw();
});
canvas.addEventListener('pointerup', (e) => {
  if (canvas.hasPointerCapture(e.pointerId)) canvas.releasePointerCapture(e.pointerId);
  manualProx = null;
});

/** Ring square fits its grid row; chart uses full plot width (no vertical overflow). */
function syncVizPlotLayout() {
  const col = document.querySelector('.viz-column');
  const ringPanel = document.querySelector('.ring-panel');
  const ringBlock = document.getElementById('ring-plot-block');
  if (!col || !diagramWrap) return;
  const plotW = Math.floor(col.getBoundingClientRect().width);
  if (plotW < 24) return;
  let ringSide = plotW;
  if (ringPanel && ringBlock) {
    const head = ringBlock.querySelector('.panel-head');
    const blockGap = 6;
    const overhead = (head ? head.offsetHeight : 0) + blockGap;
    const availH = Math.floor(ringPanel.clientHeight - overhead);
    if (availH > 24) ringSide = Math.min(plotW, availH);
  }
  diagramWrap.style.width = '100%';
  diagramWrap.style.height = ringSide + 'px';
  diagramWrap.style.maxHeight = '100%';
}

function layoutAll() {
  syncVizPlotLayout();
  sizeRingCanvas();
  redraw();
  drawOutputChart();
}

function tick() {
  const now = performance.now();
  if (trail.length) {
    while (trail.length && now - trail[0].t > 1600) trail.shift();
    redraw();
  }
  animId = requestAnimationFrame(tick);
}

window.addEventListener('resize', layoutAll);
if (typeof ResizeObserver !== 'undefined') {
  const ro = new ResizeObserver(() => layoutAll());
  const vizColumn = document.querySelector('.viz-column');
  const appScroll = document.querySelector('.app-scroll');
  if (vizColumn) ro.observe(vizColumn);
  if (appScroll) ro.observe(appScroll);
  if (diagramWrap) ro.observe(diagramWrap);
  const ringPanel = document.querySelector('.ring-panel');
  if (ringPanel) ro.observe(ringPanel);
  const chartPanel = document.getElementById('headpat-panel');
  if (chartPanel) ro.observe(chartPanel);
  document.querySelectorAll('.chart-wrap').forEach((el) => ro.observe(el));
  ro.observe(document.body);
}
requestAnimationFrame(layoutAll);
updateModeText();
updateHeadpatPanel();
tick();
</script>
</body>
</html>"#;
