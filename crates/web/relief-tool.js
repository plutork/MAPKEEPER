import {
  state, axialKey, diskCountEstimate,
} from "./workspace-state.js";
import {
  probe_max_brush_radius, probe_pulse_interval_ms, probe_next_relief,
  probe_next_relief_absolute, probe_smooth_relief_average,
} from "./wasm-api.js";
import { ensureCenterCache, expandDirtyWorld, drawSpatial, flushDrawSpatial } from "./renderer.js";
import { diskCellsAt, hexLine } from "./brush-geometry.js";
import { setHoverReadout } from "./hover-readout.js";

export { diskCellsAt } from "./brush-geometry.js";

/** Matches core AXIAL_NEIGHBOR_OFFSETS (layout offsets, not elevation rules). */
const AXIAL_NEIGHBOR_OFFSETS = [
  [1, 0],
  [1, -1],
  [0, -1],
  [-1, 0],
  [-1, 1],
  [0, 1],
];

const gridSize = () => {
  const { width, height } = state.spatial.state.grid;
  return { width, height };
};

/** Domain owns Raise/Lower (N-030); undefined means no change. */
export const nextElevation = (current, delta) => {
  const next = probe_next_relief(current, delta, state.editOcean);
  return next === undefined ? null : next;
};

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

const elevationAt = (q, r) => {
  if (!state.spatial) return 0;
  const key = axialKey(q, r);
  if (state.paintStroke?.updates?.has(key)) return state.paintStroke.updates.get(key);
  return state.spatial.state.field.cells[key] || 0;
};

const inGrid = (q, r) => {
  if (!state.spatial) return false;
  const { width, height } = state.spatial.state.grid;
  const col = q + ((r - (r & 1)) / 2 | 0);
  return col >= 0 && r >= 0 && col < width && r < height;
};

const nextForOp = (q, r, current) => {
  const op = state.paintStroke.op;
  if (op === "flatten") {
    const next = probe_next_relief_absolute(current, state.paintStroke.sample, state.editOcean);
    return next === undefined ? null : next;
  }
  if (op === "smooth") {
    const neighbors = [];
    for (const [dq, dr] of AXIAL_NEIGHBOR_OFFSETS) {
      const nq = q + dq;
      const nr = r + dr;
      if (!inGrid(nq, nr)) continue;
      neighbors.push(elevationAt(nq, nr));
    }
    const target = probe_smooth_relief_average(current, neighbors);
    const next = probe_next_relief_absolute(current, target, state.editOcean);
    return next === undefined ? null : next;
  }
  return nextElevation(current, state.paintStroke.delta);
};

export const stampDiskAt = (centerQ, centerR) => {
  if (!state.paintStroke || !state.spatial) return;
  const field = state.spatial.state.field;
  const { grid } = state.spatial.state;
  const cache = ensureCenterCache();
  const radius = state.paintStroke.radius;
  const padM = (radius + 1.5) * grid.neighbor_center_distance_m;
  const { width, height } = gridSize();
  for (const cell of diskCellsAt(centerQ, centerR, radius, width, height)) {
    const key = axialKey(cell.q, cell.r);
    if (state.paintStroke.visited.has(key)) continue;
    state.paintStroke.visited.add(key);
    const current = state.paintStroke.updates.has(key)
      ? state.paintStroke.updates.get(key)
      : (field.cells[key] || 0);
    const next = nextForOp(cell.q, cell.r, current);
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

const refreshDetailsAtBrush = () => {
  if (!state.paintStroke || !state.paintStroke.onMap) return;
  const { q, r } = state.paintStroke.last;
  setHoverReadout({ q, r }, elevationAt(q, r));
};

export const beginAirbrushEpoch = () => {
  if (!state.paintStroke || state.paintStroke.mode !== "airbrush") return;
  state.paintStroke.visited = new Set();
  const c = state.paintStroke.last;
  stampDiskAt(c.q, c.r);
  drawSpatial();
  refreshDetailsAtBrush();
};

export const beginPaintStroke = (q, r) => {
  const op = state.reliefOp || "raise";
  const mode = state.strokeMode;
  const rate = state.airbrushRate;
  const interval = mode === "airbrush" ? probe_pulse_interval_ms(rate) : 0;
  if (!state.offscreenCache) flushDrawSpatial();
  state.dirtyRect = null;
  state.paintStroke = {
    mode,
    rate,
    op,
    delta: state.reliefDirection,
    sample: elevationAt(q, r),
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
    strokeAirbrushBtn.disabled = false;
    strokeAirbrushBtn.classList.toggle("active", state.strokeMode === "airbrush");
    strokeAirbrushBtn.setAttribute(
      "aria-pressed",
      state.strokeMode === "airbrush" ? "true" : "false",
    );
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
    const op = state.reliefOp || "raise";
    const opHint = op !== "raise" && op !== "lower" ? ` · ${op}` : "";
    const modeHint = state.strokeMode === "airbrush"
      ? ` · Airbrush ${state.airbrushRate}/s`
      : "";
    brushRadiusMetaEl.textContent =
      `r=${state.brushRadius} · ~${diskCountEstimate(state.brushRadius)} cells · max ${state.brushMaxRadius} · [ ] to nudge${opHint}${modeHint}`;
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
