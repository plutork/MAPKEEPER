import { state, axialKey, radiusStepSize, diskCountEstimate, reliefMode, editOceanOn, ELEV_MIN, ELEV_MAX } from "./workspace-state.js";
import { probe_disk_cells, probe_hex_distance, probe_max_brush_radius, probe_pulse_interval_ms } from "./wasm-api.js";
import { ensureCenterCache, expandDirtyWorld, drawSpatial, flushDrawSpatial } from "./renderer.js";
import { nextElevationValue } from "./shell-math.js";

export const diskCellsAt = (q, r, radius) => {
  if (!state.spatial) return [];
  const { width, height } = state.spatial.state.grid;
  const flat = probe_disk_cells(q, r, radius, width, height);
  const cells = [];
  for (let i = 0; i + 1 < flat.length; i += 2) cells.push({ q: flat[i], r: flat[i + 1] });
  return cells;
};

export const hexLine = (a, b) => {
  const n = probe_hex_distance(a.q, a.r, b.q, b.r);
  if (n === 0) return [{ q: a.q, r: a.r }];
  const out = [];
  for (let i = 0; i <= n; i++) {
    const t = i / n;
    const x = a.q + (b.q - a.q) * t;
    const z = a.r + (b.r - a.r) * t;
    const y = -a.q - a.r + (-b.q - b.r - (-a.q - a.r)) * t;
    let rx = Math.round(x);
    let ry = Math.round(y);
    let rz = Math.round(z);
    const xDiff = Math.abs(rx - x);
    const yDiff = Math.abs(ry - y);
    const zDiff = Math.abs(rz - z);
    if (xDiff > yDiff && xDiff > zDiff) rx = -ry - rz;
    else if (yDiff > zDiff) ry = -rx - rz;
    else rz = -rx - ry;
    out.push({ q: rx, r: rz });
  }
  return out;
};

export const nextElevation = (current, delta) =>
  nextElevationValue(current, delta, {
    editOcean: editOceanOn(),
    elevMin: ELEV_MIN,
    elevMax: ELEV_MAX,
  });

export const refreshBrushMaxFromGrid = () => {
  if (!state.spatial) {
    state.brushMaxRadius = 24;
  } else {
    const { width, height } = state.spatial.state.grid;
    state.brushMaxRadius = probe_max_brush_radius(width, height);
  }
  if (state.brushRadius > state.brushMaxRadius) state.brushRadius = state.brushMaxRadius;
};

const cellIndexAt = (q, r) => {
  const cache = state.centerCache;
  if (!cache || !state.spatial) return -1;
  const { grid } = state.spatial.state;
  const col = q + ((r - (r & 1)) / 2 | 0);
  if (col < 0 || r < 0 || col >= grid.width || r >= grid.height) return -1;
  return r * grid.width + col;
};

export const stampDiskAt = (centerQ, centerR) => {
  if (!state.paintStroke || !state.spatial) return;
  const field = state.spatial.state.field;
  const { grid } = state.spatial.state;
  const cache = ensureCenterCache();
  const delta = state.paintStroke.delta;
  const radius = state.paintStroke.radius;
  const padM = (radius + 1.5) * grid.neighbor_center_distance_m;
  for (const cell of diskCellsAt(centerQ, centerR, radius)) {
    const key = axialKey(cell.q, cell.r);
    if (state.paintStroke.visited.has(key)) continue;
    state.paintStroke.visited.add(key);
    const current = state.paintStroke.updates.has(key)
      ? state.paintStroke.updates.get(key)
      : (field.cells[key] || 0);
    const next = nextElevation(current, delta);
    if (next == null) continue;
    state.paintStroke.updates.set(key, next);
    field.cells[key] = next;
    const idx = cellIndexAt(cell.q, cell.r);
    if (cache && idx >= 0) {
      if (cache.heights) cache.heights[idx] = next;
      expandDirtyWorld(cache.xs[idx], cache.ys[idx], padM);
    }
  }
};

export const clearAirbrushTimer = () => {
  if (state.paintStroke && state.paintStroke.timerId != null) {
    clearInterval(state.paintStroke.timerId);
    state.paintStroke.timerId = null;
  }
};

const elevationAt = (q, r) => {
  if (!state.spatial) return 0;
  return state.spatial.state.field.cells[axialKey(q, r)] || 0;
};

const refreshDetailsAtBrush = () => {
  if (!state.paintStroke || !state.paintStroke.onMap) return;
  const { q, r } = state.paintStroke.last;
  setHoverReadoutFn({ q, r }, elevationAt(q, r));
};

let setHoverReadoutFn = () => {};
export const setHoverReadoutRef = (fn) => { setHoverReadoutFn = fn; };

export const beginAirbrushEpoch = () => {
  if (!state.paintStroke || state.paintStroke.mode !== "airbrush") return;
  state.paintStroke.visited = new Set();
  const c = state.paintStroke.last;
  stampDiskAt(c.q, c.r);
  drawSpatial();
  refreshDetailsAtBrush();
};

export const beginPaintStroke = (q, r) => {
  const mode = state.strokeMode;
  const rate = state.airbrushRate;
  const interval = mode === "airbrush" ? probe_pulse_interval_ms(rate) : 0;
  if (!state.offscreenCache) flushDrawSpatial();
  state.paintStroke = {
    mode,
    rate,
    delta: reliefMode(),
    radius: state.brushRadius,
    visited: new Set(),
    updates: new Map(),
    last: { q, r },
    timerId: null,
    onMap: true,
  };
  stampDiskAt(q, r);
  state.hoverBrush = { q, r };
  if (mode === "airbrush" && interval > 0) {
    state.paintStroke.timerId = setInterval(() => {
      if (!state.paintStroke || state.paintStroke.mode !== "airbrush") return;
      if (!state.paintStroke.onMap) return;
      beginAirbrushEpoch();
    }, interval);
  }
  drawSpatial();
  refreshDetailsAtBrush();
};

export const extendPaintStroke = (q, r) => {
  if (!state.paintStroke) return;
  for (const p of hexLine(state.paintStroke.last, { q, r })) stampDiskAt(p.q, p.r);
  state.paintStroke.last = { q, r };
  state.paintStroke.onMap = true;
  state.hoverBrush = { q, r };
  drawSpatial();
  refreshDetailsAtBrush();
};

// Brush UI sync helpers
export const syncStrokeModeUi = () => {
  const strokeStampBtn = document.querySelector("#stroke-stamp");
  const strokeAirbrushBtn = document.querySelector("#stroke-airbrush");
  const airbrushRateBlock = document.querySelector("#airbrush-rate-block");
  const airbrushRateMetaEl = document.querySelector("#airbrush-rate-meta");
  if (strokeStampBtn) {
    strokeStampBtn.classList.toggle("active", state.strokeMode === "stamp");
    strokeStampBtn.setAttribute("aria-pressed", state.strokeMode === "stamp" ? "true" : "false");
  }
  if (strokeAirbrushBtn) {
    strokeAirbrushBtn.classList.toggle("active", state.strokeMode === "airbrush");
    strokeAirbrushBtn.setAttribute("aria-pressed", state.strokeMode === "airbrush" ? "true" : "false");
  }
  if (airbrushRateBlock) {
    airbrushRateBlock.classList.toggle("hidden", state.strokeMode !== "airbrush");
  }
  if (airbrushRateMetaEl) {
    airbrushRateMetaEl.textContent = `Airbrush · ${state.airbrushRate}/s`;
  }
  document.querySelectorAll("#airbrush-rate-stack button").forEach((btn) => {
    const on = Number(btn.dataset.rate) === state.airbrushRate;
    btn.classList.toggle("active", on);
    btn.setAttribute("aria-pressed", on ? "true" : "false");
  });
};

export const syncBrushRadiusUi = () => {
  state.brushRadius = Math.max(0, Math.min(state.brushMaxRadius, state.brushRadius | 0));
  const brushRadiusEl = document.querySelector("#brush-radius");
  const brushRadiusNumEl = document.querySelector("#brush-radius-num");
  const brushRadiusMetaEl = document.querySelector("#brush-radius-meta");
  if (brushRadiusEl) {
    brushRadiusEl.max = String(state.brushMaxRadius);
    brushRadiusEl.value = String(state.brushRadius);
  }
  if (brushRadiusNumEl) {
    brushRadiusNumEl.max = String(state.brushMaxRadius);
    brushRadiusNumEl.value = String(state.brushRadius);
  }
  if (brushRadiusMetaEl) {
    const modeHint = state.strokeMode === "airbrush" ? ` · Airbrush ${state.airbrushRate}/s` : "";
    brushRadiusMetaEl.textContent =
      `r=${state.brushRadius} · ~${diskCountEstimate(state.brushRadius)} cells · max ${state.brushMaxRadius} · [ ] to nudge${modeHint}`;
  }
  syncStrokeModeUi();
};

export const setBrushRadius = (next) => {
  state.brushRadius = Math.max(0, Math.min(state.brushMaxRadius, next | 0));
  syncBrushRadiusUi();
  drawSpatial();
};

export const setStrokeMode = (mode) => {
  state.strokeMode = mode === "airbrush" ? "airbrush" : "stamp";
  syncBrushRadiusUi();
  drawSpatial();
};

export const setAirbrushRate = (rate) => {
  const allowed = [1, 5, 10, 20];
  state.airbrushRate = allowed.includes(rate) ? rate : 5;
  syncStrokeModeUi();
  syncBrushRadiusUi();
};
