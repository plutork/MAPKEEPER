import { state, setMapLayer, radiusStepSize } from "./workspace-state.js";
import { initCamera, fitCamera, screenToWorld } from "./camera.js";
import {
  drawSpatial, flushDrawSpatial, invalidateMapCache,
  ensureCenterCache, visibleCells, setGetDiskCells,
} from "./renderer.js";
import {
  diskCellsAt, beginPaintStroke, extendPaintStroke, beginAirbrushEpoch,
  clearAirbrushTimer, setBrushRadius, setStrokeMode, setAirbrushRate,
  syncBrushRadiusUi, setHoverReadoutRef,
} from "./relief-tool.js";
import { endPaintStroke, loadSpatial } from "./spatial-transaction.js";
import { bindWorldEvents, refresh } from "./worlds.js";
import { probe_world_to_axial } from "./wasm-api.js";

// Wire cross-module callbacks
setGetDiskCells(diskCellsAt);

const canvas = document.querySelector("#spatial-canvas");
const ctx = canvas.getContext("2d");
initCamera(canvas, ctx);

const spatialStatus = document.querySelector("#spatial-status");
const hoverReliefEl = document.querySelector("#hover-relief");
const detailsLeadEl = document.querySelector("#details-lead");
const detailsCellIdEl = document.querySelector("#details-cell-id");
const detailsCellElevEl = document.querySelector("#details-cell-elev");
const viewToolPanel = document.querySelector("#view-tool-panel");
const reliefToolPanel = document.querySelector("#relief-tool-panel");
const editorToolStrip = document.querySelector("#editor-tool-strip");
const modeToolStub = document.querySelector("#mode-tool-stub");
const modeStub = document.querySelector("#mode-stub");

const syncDetailsPanel = () => {
  const reliefDetails = state.workspaceMode === "editor" && state.activeTool === "relief";
  detailsCellIdEl.classList.toggle("hidden", !reliefDetails);
  detailsCellElevEl.classList.toggle("hidden", !reliefDetails);
  if (!reliefDetails) {
    detailsLeadEl.textContent = "Elevation cell readout appears in Relief.";
    detailsCellIdEl.textContent = "Cell: —";
    detailsCellElevEl.textContent = "Elevation: —";
  } else {
    detailsLeadEl.textContent = "Hover a cell while painting Relief.";
  }
};

const setHoverReadout = (axial, h, { outside = false } = {}) => {
  if (outside || !axial) {
    hoverReliefEl.textContent = "Elevation: —";
    if (state.activeTool === "relief") {
      detailsCellIdEl.textContent = "Cell: —";
      detailsCellElevEl.textContent = "Elevation: —";
    }
    return;
  }
  hoverReliefEl.textContent = `Elevation: ${h} @ ${axial.q},${axial.r}`;
  if (state.activeTool !== "relief") return;
  detailsCellIdEl.textContent = `Cell: ${axial.q}, ${axial.r}`;
  detailsCellElevEl.textContent = `Elevation: ${h}`;
};

setHoverReadoutRef(setHoverReadout);

const syncEditorChrome = () => {
  const isEditor = state.workspaceMode === "editor";
  editorToolStrip.classList.toggle("hidden", !isEditor);
  modeToolStub.classList.toggle("hidden", isEditor);
  modeStub.classList.toggle("hidden", isEditor);
  viewToolPanel.classList.toggle("hidden", !isEditor || state.activeTool !== "view");
  reliefToolPanel.classList.toggle("hidden", !isEditor || state.activeTool !== "relief");
  const title = !isEditor
    ? state.workspaceMode[0].toUpperCase() + state.workspaceMode.slice(1)
    : (state.activeTool === "relief" ? "Relief" : "View");
  document.querySelector("#left-title").textContent = title;
  canvas.classList.toggle("paint", isEditor && state.activeTool === "relief");
  document.querySelectorAll("#editor-tool-strip button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.tool === state.activeTool);
  });
  syncDetailsPanel();
};

const setTool = (tool) => {
  state.activeTool = tool;
  if (tool === "relief") setMapLayer("relief");
  if (tool !== "relief") state.hoverBrush = null;
  syncEditorChrome();
  invalidateMapCache();
  drawSpatial();
};

const pickAxial = (clientX, clientY) => {
  const rect = canvas.getBoundingClientRect();
  const [wx, wy] = screenToWorld(clientX - rect.left, clientY - rect.top);
  const { frame, grid } = state.spatial.state;
  return probe_world_to_axial(
    frame.origin_x, frame.origin_y,
    grid.neighbor_center_distance_m, grid.width, grid.height, wx, wy,
  );
};

const axialOnMap = (axial) => {
  const { grid } = state.spatial.state;
  const col = axial.q + ((axial.r - (axial.r & 1)) / 2 | 0);
  const row = axial.r;
  return col >= 0 && row >= 0 && col < grid.width && row < grid.height;
};

// --- Event binding ---

document.querySelector("#tool-view").addEventListener("click", () => setTool("view"));
document.querySelector("#tool-relief").addEventListener("click", () => setTool("relief"));

document.querySelector("#mode-nav").addEventListener("click", (event) => {
  const button = event.target.closest("button[data-mode]");
  if (!button) return;
  document.querySelectorAll("#mode-nav button").forEach((item) => item.classList.toggle("active", item === button));
  state.workspaceMode = button.dataset.mode;
  syncEditorChrome();
});

canvas.addEventListener("mousedown", (event) => {
  if (!state.spatial || state.workspaceMode !== "editor" || event.button !== 0) return;
  if (state.activeTool === "view") {
    state.panDrag = {
      x: event.clientX,
      y: event.clientY,
      cx: state.camera.cx,
      cy: state.camera.cy,
      moved: false,
    };
    canvas.style.cursor = "grabbing";
    event.preventDefault();
    return;
  }
  if (state.activeTool !== "relief") return;
  const axial = pickAxial(event.clientX, event.clientY);
  if (!axialOnMap(axial)) return;
  event.preventDefault();
  beginPaintStroke(axial.q, axial.r);
});

window.addEventListener("mousemove", (event) => {
  if (state.paintStroke) {
    const axial = pickAxial(event.clientX, event.clientY);
    if (axialOnMap(axial)) {
      extendPaintStroke(axial.q, axial.r);
    } else {
      state.paintStroke.onMap = false;
    }
    return;
  }
  if (!state.panDrag) return;
  const dx = event.clientX - state.panDrag.x;
  const dy = event.clientY - state.panDrag.y;
  if (Math.abs(dx) + Math.abs(dy) > 2) state.panDrag.moved = true;
  state.camera.cx = state.panDrag.cx - dx / state.camera.zoom;
  state.camera.cy = state.panDrag.cy - dy / state.camera.zoom;
  drawSpatial();
});

window.addEventListener("mouseup", async () => {
  if (state.paintStroke) {
    await endPaintStroke();
    return;
  }
  if (!state.panDrag) return;
  state.panDrag = null;
  canvas.style.cursor = "";
  syncEditorChrome();
});

window.addEventListener("blur", async () => {
  if (state.paintStroke) await endPaintStroke();
});

canvas.addEventListener("wheel", (event) => {
  if (!state.spatial || state.workspaceMode !== "editor") return;
  event.preventDefault();
  const rect = canvas.getBoundingClientRect();
  const sx = event.clientX - rect.left;
  const sy = event.clientY - rect.top;
  const [wx, wy] = screenToWorld(sx, sy);
  const factor = event.deltaY < 0 ? 1.12 : 1 / 1.12;
  state.camera.zoom = Math.max(0.001, Math.min(2, state.camera.zoom * factor));
  const [wx2, wy2] = screenToWorld(sx, sy);
  state.camera.cx += wx - wx2;
  state.camera.cy += wy - wy2;
  invalidateMapCache();
  drawSpatial();
}, { passive: false });

canvas.addEventListener("mousemove", (event) => {
  if (!state.spatial || state.workspaceMode !== "editor" || state.paintStroke) return;
  const axial = pickAxial(event.clientX, event.clientY);
  const onMap = axialOnMap(axial);
  if (!onMap) {
    setHoverReadout(null, 0, { outside: true });
    if (state.hoverBrush) {
      state.hoverBrush = null;
      drawSpatial();
    }
    return;
  }
  const key = `${axial.q},${axial.r}`;
  const h = state.spatial.state.field.cells[key] || 0;
  setHoverReadout(axial, h);
  if (state.activeTool === "relief") {
    if (!state.hoverBrush || state.hoverBrush.q !== axial.q || state.hoverBrush.r !== axial.r) {
      state.hoverBrush = { q: axial.q, r: axial.r };
      drawSpatial();
    }
  } else if (state.hoverBrush) {
    state.hoverBrush = null;
    drawSpatial();
  }
});

canvas.addEventListener("mouseleave", () => {
  if (state.paintStroke) return;
  if (state.hoverBrush) {
    state.hoverBrush = null;
    drawSpatial();
  }
});

const brushRadiusEl = document.querySelector("#brush-radius");
const brushRadiusNumEl = document.querySelector("#brush-radius-num");
if (brushRadiusEl) {
  brushRadiusEl.addEventListener("input", () => setBrushRadius(Number(brushRadiusEl.value)));
}
if (brushRadiusNumEl) {
  brushRadiusNumEl.addEventListener("change", () => setBrushRadius(Number(brushRadiusNumEl.value)));
}
document.querySelector("#stroke-stamp")?.addEventListener("click", () => setStrokeMode("stamp"));
document.querySelector("#stroke-airbrush")?.addEventListener("click", () => setStrokeMode("airbrush"));
document.querySelector("#airbrush-rate-stack")?.addEventListener("click", (event) => {
  const btn = event.target.closest("button[data-rate]");
  if (!btn) return;
  setAirbrushRate(Number(btn.dataset.rate));
});

window.addEventListener("keydown", (event) => {
  if (state.workspaceMode !== "editor" || state.activeTool !== "relief") return;
  const tag = (event.target && event.target.tagName) || "";
  if (tag === "INPUT" || tag === "TEXTAREA") return;
  if (event.key === "[") {
    event.preventDefault();
    setBrushRadius(state.brushRadius - radiusStepSize(state.brushRadius));
  } else if (event.key === "]") {
    event.preventDefault();
    const step = radiusStepSize(state.brushRadius);
    setBrushRadius(state.brushRadius + (state.brushRadius === 0 ? 1 : step));
  }
});

document.querySelector("#reload-spatial").addEventListener("click", () => {
  loadSpatial().catch((error) => { spatialStatus.textContent = error.message; });
});

document.querySelector("#map-layer-stack").addEventListener("click", (event) => {
  const button = event.target.closest("button[data-layer]");
  if (!button) return;
  setMapLayer(button.dataset.layer);
  invalidateMapCache();
  drawSpatial();
});

document.querySelector("#view-hex-grid").addEventListener("click", (event) => {
  const btn = event.currentTarget;
  const on = !btn.classList.contains("active");
  btn.classList.toggle("active", on);
  btn.setAttribute("aria-pressed", on ? "true" : "false");
  invalidateMapCache();
  drawSpatial();
});

document.querySelector("#edit-ocean").addEventListener("click", (event) => {
  const btn = event.currentTarget;
  const on = !btn.classList.contains("active");
  btn.classList.toggle("active", on);
  btn.setAttribute("aria-pressed", on ? "true" : "false");
});

window.addEventListener("resize", () => {
  if (state.spatial) {
    fitCamera();
    ensureCenterCache();
    invalidateMapCache();
  }
  drawSpatial();
});

// --- Bench hooks (N-026) ---
const percentile = (arr, p) => {
  if (!arr.length) return null;
  const s = [...arr].sort((a, b) => a - b);
  const i = Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1));
  return s[i];
};

const measureFrames = async (n, tick) => {
  const samples = [];
  for (let i = 0; i < n; i++) {
    const t0 = performance.now();
    tick();
    flushDrawSpatial();
    samples.push(performance.now() - t0);
    await new Promise((r) => requestAnimationFrame(r));
  }
  return {
    p50: percentile(samples, 50),
    p95: percentile(samples, 95),
    n: samples.length,
  };
};

window.__MK_BENCH__ = {
  lastFrameMs: () => state.lastFrameMs,
  flushDraw: () => flushDrawSpatial(),
  ensureCenters: () => ensureCenterCache(),
  visibleCount: () => visibleCells(64, { useDirty: false }).length,
  cellCount: () => (state.spatial ? state.spatial.state.grid.width * state.spatial.state.grid.height : 0),
  async runSuite() {
    if (!state.spatial) throw new Error("no active world");
    const out = { cells: this.cellCount(), revision: state.spatial.state.revision };
    const cloneCells = () => JSON.parse(JSON.stringify(state.spatial.state.field.cells));
    const restoreCells = (cells) => {
      state.spatial.state.field.cells = JSON.parse(JSON.stringify(cells));
      state.heightRev = -1;
      ensureCenterCache();
      invalidateMapCache();
    };
    setMapLayer("empty");
    invalidateMapCache();
    flushDrawSpatial();
    out.open_fit = await measureFrames(8, () => { fitCamera(); ensureCenterCache(); invalidateMapCache(); });
    out.pan = await measureFrames(12, () => {
      state.camera.cx += state.spatial.state.grid.neighbor_center_distance_m * 0.25;
    });
    out.zoom = await measureFrames(8, () => {
      state.camera.zoom *= 1.05;
    });
    fitCamera();
    ensureCenterCache();
    invalidateMapCache();
    flushDrawSpatial();
    out.hover = await measureFrames(12, () => {
      state.hoverBrush = { q: 2, r: 2 };
    });
    out.view_empty = await measureFrames(6, () => { setMapLayer("empty"); invalidateMapCache(); });
    out.relief = await measureFrames(6, () => { setMapLayer("relief"); invalidateMapCache(); });
    state.activeTool = "relief";
    state.brushRadius = Math.min(3, state.brushMaxRadius);
    out.radius_preview = await measureFrames(8, () => {
      state.hoverBrush = { q: 4, r: 4 };
    });
    const snapCells = cloneCells();
    const stampSamples = [];
    for (let i = 0; i < 8; i++) {
      restoreCells(snapCells);
      flushDrawSpatial();
      beginPaintStroke(3 + i, 3);
      const t0 = performance.now();
      extendPaintStroke(5 + i, 4);
      flushDrawSpatial();
      stampSamples.push(performance.now() - t0);
      clearAirbrushTimer();
      state.paintStroke = null;
    }
    out.stamp_drag = {
      p50: percentile(stampSamples, 50),
      p95: percentile(stampSamples, 95),
      n: stampSamples.length,
    };
    out.airbrush = {};
    for (const rate of [1, 5, 10, 20]) {
      state.airbrushRate = rate;
      state.strokeMode = "airbrush";
      restoreCells(snapCells);
      flushDrawSpatial();
      beginPaintStroke(6, 6);
      flushDrawSpatial();
      const samples = [];
      const pulses = Math.min(8, Math.max(3, Math.ceil(rate / 2)));
      for (let p = 0; p < pulses; p++) {
        const t0 = performance.now();
        beginAirbrushEpoch();
        flushDrawSpatial();
        samples.push(performance.now() - t0);
      }
      clearAirbrushTimer();
      state.paintStroke = null;
      out.airbrush[String(rate)] = {
        p50: percentile(samples, 50),
        p95: percentile(samples, 95),
        n: samples.length,
      };
    }
    restoreCells(snapCells);
    state.strokeMode = "stamp";
    state.airbrushRate = 5;
    state.activeTool = "view";
    state.hoverBrush = null;
    fitCamera();
    ensureCenterCache();
    invalidateMapCache();
    flushDrawSpatial();
    out.visible_at_fit = this.visibleCount();
    out.crs = {
      centerCache: !!state.centerCache,
      offscreenCache: !!state.offscreenCache,
      viewportCull: true,
      rafCoalesce: true,
      dirtyRect: true,
    };
    return out;
  },
};

// --- Bootstrap ---
bindWorldEvents();
syncBrushRadiusUi();
syncEditorChrome();
refresh().catch((error) => {
  document.querySelector("#project-empty").textContent = error.message;
});
