(function () {
  'use strict';

  const VEL_HISTORY_LEN = 100;
  const CHART_SCALE_MAX = 100;
  const TRAIL_MAX = 48;
  const TRAIL_ANGLE = -Math.PI / 2;
  /** Touch ring canvas targets 2× the default layout size when space allows. */
  const RING_LAYOUT_SCALE = 2;
  const RING_LAYOUT_SCALE_VR = 4;

  function ringLayoutScale() {
    return document.body && document.body.classList.contains('ui-large')
      ? RING_LAYOUT_SCALE_VR
      : RING_LAYOUT_SCALE;
  }

  const instances = new Map();
  let globalTickRaf = 0;
  let globalResizeBound = false;

  function defaultState(index) {
    return {
      index: index,
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
  }

  function ensureGlobalTick() {
    if (globalTickRaf) return;
    globalTickRaf = requestAnimationFrame(globalTick);
  }

  function stopGlobalTickIfEmpty() {
    if (instances.size === 0 && globalTickRaf) {
      cancelAnimationFrame(globalTickRaf);
      globalTickRaf = 0;
    }
  }

  function globalTick() {
    globalTickRaf = 0;
    const now = performance.now();
    for (const inst of instances.values()) {
      if (inst.trail.length) {
        while (inst.trail.length && now - inst.trail[0].t > 1600) inst.trail.shift();
        inst.needRingPaint = true;
      }
      if (inst.needRingPaint || inst.needChartPaint) {
        schedulePaint(inst);
      }
    }
    if (instances.size > 0) {
      globalTickRaf = requestAnimationFrame(globalTick);
    }
  }

  function scheduleGlobalLayout() {
    for (const inst of instances.values()) {
      scheduleLayoutAll(inst);
    }
  }

  function bindGlobalResize() {
    if (globalResizeBound) return;
    globalResizeBound = true;
    window.addEventListener('resize', scheduleGlobalLayout);
  }

  function schedulePaint(inst) {
    if (inst.paintRaf) return;
    inst.paintRaf = requestAnimationFrame(() => {
      inst.paintRaf = 0;
      if (inst.needRingPaint) {
        inst.needRingPaint = false;
        redraw(inst);
      }
      if (inst.needChartPaint) {
        inst.needChartPaint = false;
        drawOutputChart(inst);
      }
    });
  }

  function scheduleRingPaint(inst) {
    inst.needRingPaint = true;
    schedulePaint(inst);
  }

  function scheduleChartPaint(inst) {
    inst.needChartPaint = true;
    schedulePaint(inst);
  }

  function displayProx(inst) {
    if (inst.manualProx != null) return inst.manualProx;
    if (inst.liveProx != null) return inst.liveProx;
    return 0;
  }

  function proxInBand(p, s) {
    if (p < s.outer) return false;
    if (s.velocity) return p <= s.inner;
    return true;
  }

  function pushTrail(inst, p) {
    const t = performance.now();
    inst.trail.push({ p: p, t: t });
    while (inst.trail.length > TRAIL_MAX) inst.trail.shift();
  }

  function proxToRadius(p, maxR) {
    return (1 - Math.max(0, Math.min(1, p))) * maxR;
  }

  function snap(v) {
    return Math.round(v * 2) / 2;
  }

  function fillAnnulus(ctx, cx, cy, rOuter, rInner, color) {
    if (rOuter <= rInner + 0.5) return;
    ctx.beginPath();
    ctx.arc(cx, cy, snap(rOuter), 0, Math.PI * 2, false);
    ctx.arc(cx, cy, snap(rInner), 0, Math.PI * 2, true);
    ctx.fillStyle = color;
    ctx.fill('evenodd');
  }

  function strokeCircle(ctx, cx, cy, r, color, lineW) {
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

  function fillCircle(ctx, cx, cy, r, color) {
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

  function sizeRingCanvas(inst) {
    if (!inst.diagramWrap || !inst.canvas) return 0;
    const rect = inst.diagramWrap.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const cssSide = Math.floor(Math.min(rect.width, rect.height));
    if (cssSide < 24) return 0;
    const side = Math.floor(cssSide * dpr);
    inst.canvas.style.width = cssSide + 'px';
    inst.canvas.style.height = cssSide + 'px';
    if (inst.canvas.width !== side || inst.canvas.height !== side) {
      inst.canvas.width = side;
      inst.canvas.height = side;
    }
    return side;
  }

  function redraw(inst) {
    const size = sizeRingCanvas(inst);
    if (!size || !inst.ctx) return;
    const ctx = inst.ctx;
    const dpr = window.devicePixelRatio || 1;
    const w = size;
    const h = size;
    const cx = snap(w / 2);
    const cy = snap(h / 2);
    const maxR = snap(w * 0.38);
    const lineW = Math.max(2, 2 * dpr);

    const rOuter = snap(proxToRadius(inst.state.outer, maxR));
    const rInner = snap(proxToRadius(inst.state.inner, maxR));
    const rMax = maxR;
    const rMaxCenter = snap(10 * dpr);
    const p = Math.max(0, Math.min(1, displayProx(inst)));
    const rSample = snap(proxToRadius(p, maxR));

    ctx.setTransform(1, 0, 0, 1, 0, 0);
    ctx.imageSmoothingEnabled = true;
    ctx.clearRect(0, 0, w, h);

    fillAnnulus(ctx, cx, cy, rMax, rOuter, '#1a1a24');
    fillAnnulus(ctx, cx, cy, rOuter, rInner, 'rgba(45, 106, 79, 0.4)');
    fillCircle(ctx, cx, cy, rInner, 'rgba(92, 74, 26, 0.38)');

    const now = performance.now();
    for (let i = 0; i < inst.trail.length; i++) {
      const pt = inst.trail[i];
      const age = (now - pt.t) / 1400;
      const alpha = Math.max(0.06, 1 - age);
      const r = snap(proxToRadius(pt.p, maxR));
      const ptXY = pointOnRing(cx, cy, r, TRAIL_ANGLE);
      fillCircle(
        ctx,
        ptXY.x,
        ptXY.y,
        snap(3 * dpr + alpha * 2 * dpr),
        'rgba(110, 231, 168, ' + alpha * 0.5 + ')'
      );
    }

    strokeCircle(ctx, cx, cy, rOuter, '#4a7cff', lineW);
    if (rInner >= rMaxCenter + 2) strokeCircle(ctx, cx, cy, rInner, '#ff9f43', lineW);

    const inBand = proxInBand(p, inst.state);
    const atZero = p <= 0.001;
    const sample = pointOnRing(cx, cy, rSample, TRAIL_ANGLE);
    const dotR = snap(5 * dpr);
    const dotFill = atZero ? '#e85d5d' : inBand ? '#6ee7a8' : '#5a5a6e';
    const dotStroke = atZero ? '#f0a0a0' : inBand ? '#e8e8f0' : '#f0a0a0';
    fillCircle(ctx, sample.x, sample.y, dotR, dotFill);
    strokeCircle(ctx, sample.x, sample.y, dotR, dotStroke, lineW * 0.85);
  }

  function toChartPct(v) {
    return Math.min(CHART_SCALE_MAX, Math.max(0, v));
  }

  function motorToChartPct(motor) {
    return Math.min(CHART_SCALE_MAX, Math.max(0, motor * 100));
  }

  function drawMotorSeriesOn(ctx, arr, color, maxV, w, h, dpr, plot) {
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
      const y = h - plot.padB - (motorToChartPct(arr[i]) / scale) * plotH;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.stroke();
  }

  function updateChartLegend(inst) {
    const legend = inst.chartLegendEl;
    if (!legend) return;
    if (inst.state.velocity) {
      let html = '<span><i class="swatch raw"></i> Touch raw</span>';
      html += '<span><i class="swatch smooth"></i> Touch smoothed</span>';
      html += '<span><i class="swatch motor"></i> Motor</span>';
      legend.innerHTML = html;
    } else {
      legend.innerHTML = '<span><i class="swatch motor"></i> Motor</span>';
    }
  }

  function pushVelHistory(inst, key, v) {
    const arr = inst.velHistory[key];
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
      padL: 12 * dpr,
      padR: 12 * dpr,
      padT: 16 * dpr,
      padB: 20 * dpr
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
    const gridStyle = 'rgba(107, 107, 128, 0.22)';
    const y100 = plot.padT;
    const y50 = plot.padT + plotH * 0.5;
    const y0 = h - plot.padB;
    drawHLine(ctx, x0, x1, y100, gridStyle, dpr, true);
    drawHLine(ctx, x0, x1, y50, gridStyle, dpr, true);
    drawHLine(ctx, x0, x1, y0, gridStyle, dpr, false);
    return plot;
  }

  function drawOutputChart(inst) {
    if (!inst.outputCtx) return;
    const dims = sizeChartCanvas(inst.outputChart);
    if (!dims) return;
    const w = dims.w;
    const h = dims.h;
    const dpr = dims.dpr;
    inst.outputCtx.clearRect(0, 0, w, h);
    const plot = drawChartAxes(inst.outputCtx, w, h, dpr);
    const maxV = plot.maxV;
    if (inst.state.velocity) {
      drawSeriesOn(inst.outputCtx, inst.velHistory.pre, '#5b8def', maxV, w, h, dpr, plot);
      drawSeriesOn(inst.outputCtx, inst.velHistory.smooth, '#ffb020', maxV, w, h, dpr, plot);
      drawMotorSeriesOn(inst.outputCtx, inst.velHistory.motor, '#e8e8f0', maxV, w, h, dpr, plot);
    } else {
      drawMotorSeriesOn(inst.outputCtx, inst.velHistory.motor, '#e8e8f0', maxV, w, h, dpr, plot);
    }
  }

  function updateHeadpatPanel(inst) {
    inst.root.classList.toggle('velocity-mode', !!inst.state.velocity);
    if (inst.chartTitleEl) {
      inst.chartTitleEl.textContent = inst.state.velocity ? 'Vibration control' : 'Motor output';
    }
    updateChartLegend(inst);
    scheduleLayoutAll(inst);
  }

  function clearVelHistory(inst) {
    inst.velHistory.pre.length = 0;
    inst.velHistory.smooth.length = 0;
    inst.velHistory.motor.length = 0;
    scheduleChartPaint(inst);
  }

  function resetColliderSession(inst) {
    inst.liveProx = null;
    inst.manualProx = null;
    inst.trail.length = 0;
    inst.lastTelemetry = null;
    clearVelHistory(inst);
  }

  function updateModeText(inst) {
    if (inst.titleEl) {
      inst.titleEl.textContent = inst.state.name || 'Device';
    }
  }

  function ingestProxSample(inst, v) {
    const p = Math.max(0, Math.min(1, Number(v) || 0));
    inst.liveProx = p;
    if (inst.manualProx == null) {
      if (inst.trail.length === 0 || Math.abs(inst.trail[inst.trail.length - 1].p - p) > 0.002) {
        pushTrail(inst, p);
      }
    }
    return p;
  }

  function ingestHeadpatTelemetry(inst, t, opts) {
    const append = !opts || opts.append !== false;
    const sample = {
      pre: Number(t.pre) || 0,
      smooth: Number(t.smooth) || 0,
      motor: Math.max(0, Math.min(1, Number(t.motor) || 0))
    };
    inst.lastTelemetry = sample;
    if (append) {
      pushVelHistory(inst, 'motor', sample.motor);
      if (inst.state.velocity) {
        pushVelHistory(inst, 'pre', toChartPct(sample.pre));
        pushVelHistory(inst, 'smooth', toChartPct(sample.smooth));
      }
    }
  }

  function pointerToProx(inst, clientX, clientY) {
    const canvas = inst.canvas;
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

  function syncVizPlotLayout(inst) {
    const col = inst.vizColumn;
    const ringPanel = inst.ringPanel;
    const ringBlock = inst.ringBlock;
    if (!col || !inst.diagramWrap) return;
    const plotW = Math.floor(col.getBoundingClientRect().width);
    if (plotW < 24) return;
    const scale = ringLayoutScale();
    let ringSide = plotW * scale;
    if (ringPanel && ringBlock) {
      const head = ringBlock.querySelector('.panel-head');
      const blockGap = 6;
      const overhead = (head ? head.offsetHeight : 0) + blockGap;
      const availH = Math.floor(ringPanel.clientHeight - overhead);
      if (availH > 24) ringSide = Math.min(plotW * scale, availH);
    }
    inst.diagramWrap.style.width = '100%';
    inst.diagramWrap.style.height = ringSide + 'px';
    inst.diagramWrap.style.maxHeight = '100%';
  }

  function layoutAll(inst) {
    syncVizPlotLayout(inst);
    inst.needRingPaint = true;
    inst.needChartPaint = true;
    schedulePaint(inst);
  }

  function scheduleLayoutAll(inst) {
    if (inst.layoutRaf) return;
    inst.layoutRaf = requestAnimationFrame(() => {
      inst.layoutRaf = 0;
      layoutAll(inst);
    });
  }

  function onPointerDown(inst, e) {
    inst.manualProx = pointerToProx(inst, e.clientX, e.clientY);
    pushTrail(inst, inst.manualProx);
    inst.canvas.setPointerCapture(e.pointerId);
    scheduleRingPaint(inst);
  }

  function onPointerMove(inst, e) {
    if (!inst.canvas.hasPointerCapture(e.pointerId)) return;
    inst.manualProx = pointerToProx(inst, e.clientX, e.clientY);
    pushTrail(inst, inst.manualProx);
    scheduleRingPaint(inst);
  }

  function onPointerUp(inst, e) {
    if (inst.canvas.hasPointerCapture(e.pointerId)) {
      inst.canvas.releasePointerCapture(e.pointerId);
    }
    inst.manualProx = null;
  }

  function wirePointerEvents(inst) {
    const canvas = inst.canvas;
    if (!canvas) return;
    inst._onPointerDown = (e) => onPointerDown(inst, e);
    inst._onPointerMove = (e) => onPointerMove(inst, e);
    inst._onPointerUp = (e) => onPointerUp(inst, e);
    canvas.addEventListener('pointerdown', inst._onPointerDown);
    canvas.addEventListener('pointermove', inst._onPointerMove);
    canvas.addEventListener('pointerup', inst._onPointerUp);
  }

  function unwirePointerEvents(inst) {
    const canvas = inst.canvas;
    if (!canvas) return;
    if (inst._onPointerDown) canvas.removeEventListener('pointerdown', inst._onPointerDown);
    if (inst._onPointerMove) canvas.removeEventListener('pointermove', inst._onPointerMove);
    if (inst._onPointerUp) canvas.removeEventListener('pointerup', inst._onPointerUp);
  }

  function wireResizeObserver(inst) {
    if (typeof ResizeObserver === 'undefined') return;
    const ro = new ResizeObserver(() => scheduleLayoutAll(inst));
    inst.resizeObserver = ro;
    ro.observe(inst.root);
    if (inst.vizColumn) ro.observe(inst.vizColumn);
    if (inst.diagramWrap) ro.observe(inst.diagramWrap);
    if (inst.ringPanel) ro.observe(inst.ringPanel);
    if (inst.chartPanel) ro.observe(inst.chartPanel);
    inst.root.querySelectorAll('.chart-wrap').forEach((el) => ro.observe(el));
  }

  function createInstance(root, index) {
    const canvas = root.querySelector('.cv-ring');
    const inst = {
      root: root,
      index: index,
      state: defaultState(index),
      velHistory: { pre: [], smooth: [], motor: [] },
      lastTelemetry: null,
      liveProx: null,
      manualProx: null,
      trail: [],
      canvas: canvas,
      ctx: canvas ? canvas.getContext('2d') : null,
      outputChart: root.querySelector('.cv-output-chart'),
      outputCtx: null,
      diagramWrap: root.querySelector('.cv-diagram-wrap'),
      chartLegendEl: root.querySelector('.cv-chart-legend'),
      chartTitleEl: root.querySelector('.cv-chart-title'),
      titleEl: root.querySelector('.cv-title'),
      vizColumn: root.querySelector('.cv-viz-column'),
      ringPanel: root.querySelector('.cv-ring-panel'),
      ringBlock: root.querySelector('.cv-ring-plot-block'),
      chartPanel: root.querySelector('.cv-chart-panel'),
      paintRaf: 0,
      needRingPaint: false,
      needChartPaint: false,
      layoutRaf: 0,
      resizeObserver: null
    };
    if (inst.outputChart) {
      inst.outputCtx = inst.outputChart.getContext('2d');
    }
    return inst;
  }

  function mount(root, index) {
    if (!root || !root.classList || !root.classList.contains('collider-viz-root')) {
      return false;
    }
    const idx = Number(index);
    if (!Number.isFinite(idx)) return false;
    unmount(idx);
    const inst = createInstance(root, idx);
    instances.set(idx, inst);
    wirePointerEvents(inst);
    wireResizeObserver(inst);
    bindGlobalResize();
    updateModeText(inst);
    updateHeadpatPanel(inst);
    ensureGlobalTick();
    return true;
  }

  function unmount(index) {
    const idx = Number(index);
    const inst = instances.get(idx);
    if (!inst) return;
    unwirePointerEvents(inst);
    if (inst.resizeObserver) {
      inst.resizeObserver.disconnect();
      inst.resizeObserver = null;
    }
    if (inst.paintRaf) {
      cancelAnimationFrame(inst.paintRaf);
      inst.paintRaf = 0;
    }
    if (inst.layoutRaf) {
      cancelAnimationFrame(inst.layoutRaf);
      inst.layoutRaf = 0;
    }
    instances.delete(idx);
    stopGlobalTickIfEmpty();
  }

  function getInstance(index) {
    return instances.get(Number(index));
  }

  function applyState(s) {
    if (!s || s.index == null) return;
    const inst = getInstance(s.index);
    if (!inst) return;
    const prevIndex = inst.state.index;
    const prevParam = inst.state.proximity_parameter;
    const prevIp = inst.state.device_ip;
    const prevVelocity = inst.state.velocity;
    inst.state = Object.assign({}, inst.state, s);
    if (inst.state.inner <= inst.state.outer) {
      inst.state.inner = Math.min(1, inst.state.outer + 0.01);
    }
    const identityChanged =
      (s.index != null && s.index !== prevIndex) ||
      (s.proximity_parameter != null && s.proximity_parameter !== prevParam) ||
      (s.device_ip != null && s.device_ip !== prevIp) ||
      (s.velocity != null && !!s.velocity !== !!prevVelocity);
    if (identityChanged) resetColliderSession(inst);
    updateModeText(inst);
    updateHeadpatPanel(inst);
    layoutAll(inst);
  }

  function applyProxSample(index, v) {
    const inst = getInstance(index);
    if (!inst) return;
    ingestProxSample(inst, v);
    scheduleRingPaint(inst);
  }

  function applyHeadpatTelemetry(index, t, opts) {
    const inst = getInstance(index);
    if (!inst) return;
    ingestHeadpatTelemetry(inst, t, opts);
    scheduleChartPaint(inst);
  }

  function applyMotorSample(index, m) {
    const inst = getInstance(index);
    if (!inst) return;
    const motor = Math.max(0, Math.min(1, Number(m) || 0));
    if (inst.lastTelemetry) {
      inst.lastTelemetry = {
        pre: inst.lastTelemetry.pre,
        smooth: inst.lastTelemetry.smooth,
        motor: motor
      };
    } else {
      inst.lastTelemetry = { pre: 0, smooth: 0, motor: motor };
    }
    pushVelHistory(inst, 'motor', motor);
    scheduleChartPaint(inst);
  }

  function applyFlush(index, prox, t, opts) {
    const inst = getInstance(index);
    if (!inst) return;
    let ring = false;
    let chart = false;
    if (typeof prox === 'number') {
      ingestProxSample(inst, prox);
      ring = true;
    }
    if (t != null && typeof t === 'object') {
      ingestHeadpatTelemetry(inst, t, opts);
      chart = true;
    }
    if (ring) inst.needRingPaint = true;
    if (chart) inst.needChartPaint = true;
    if (ring || chart) schedulePaint(inst);
  }

  window.colliderVizApi = {
    mount: mount,
    unmount: unmount,
    applyState: applyState,
    applyProxSample: applyProxSample,
    applyHeadpatTelemetry: applyHeadpatTelemetry,
    applyMotorSample: applyMotorSample,
    applyFlush: applyFlush,
    relayoutAll: scheduleGlobalLayout
  };
})();
