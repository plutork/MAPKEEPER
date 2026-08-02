import { state, ELEV_MIN, ELEV_MAX } from "./workspace-state.js";
import { probe_grid_centers, probe_axial_to_world } from "./wasm-api.js";
import { resizeCanvas, worldToScreen, getCanvas, getCtx, syncViewCameraDebug } from "./camera.js";
import {
  cellInViewport, cellInDirty, expandDirtyAabb, offscreenBlitArgs,
} from "./shell-math.js";
import { diskCellsAt } from "./brush-geometry.js";

/** Blit HiDPI cache into CSS-space CTM (explicit CSS dest, not device px). */
const blitOffscreen = (ctx, oc, dx, dy, cssW, cssH) => {
  ctx.drawImage(oc, ...offscreenBlitArgs(oc.width, oc.height, dx, dy, cssW, cssH));
};

const lerpChannel = (a, b, t) => a + (b - a) * t;

export const reliefTintRgb = (value) => {
  const h = Number(value);
  if (h < 0) {
    const depth = Math.min(1, (-h) / (-ELEV_MIN));
    return [
      Math.round(lerpChannel(56, 10, depth)),
      Math.round(lerpChannel(140, 36, depth)),
      Math.round(lerpChannel(210, 88, depth)),
    ];
  }
  const t = Math.min(1, Math.max(0, h / ELEV_MAX));
  if (t <= 0.5) {
    const u = t * 2;
    return [
      Math.round(lerpChannel(22, 250, u)),
      Math.round(lerpChannel(163, 204, u)),
      Math.round(lerpChannel(74, 21, u)),
    ];
  }
  const u = (t - 0.5) * 2;
  return [
    Math.round(lerpChannel(250, 239, u)),
    Math.round(lerpChannel(204, 68, u)),
    Math.round(lerpChannel(21, 68, u)),
  ];
};

export const invalidateMapCache = () => {
  state.offscreenCache = null;
  state.dirtyRect = null;
  if (state.cacheRebuildRaf) {
    cancelAnimationFrame(state.cacheRebuildRaf);
    state.cacheRebuildRaf = 0;
  }
};

const syncHeightGrid = () => {
  const cache = state.centerCache;
  if (!cache || !state.spatial) return;
  const rev = state.spatial.state.revision;
  const cells = state.spatial.state.field.cells;
  if (cache.heights && state.heightRev === rev && cache.heights.length === cache.n) return;
  const heights = cache.heights && cache.heights.length === cache.n
    ? cache.heights
    : new Int16Array(cache.n);
  for (let i = 0; i < cache.n; i++) {
    heights[i] = cells[`${cache.qs[i]},${cache.rs[i]}`] || 0;
  }
  cache.heights = heights;
  state.heightRev = rev;
};

export const ensureCenterCache = () => {
  if (!state.spatial) {
    state.centerCache = null;
    state.heightRev = -1;
    return null;
  }
  const { frame, grid } = state.spatial.state;
  const key = `${frame.origin_x},${frame.origin_y},${grid.neighbor_center_distance_m},${grid.width}x${grid.height}`;
  if (state.centerCache && state.centerCache.key === key) {
    syncHeightGrid();
    return state.centerCache;
  }
  const flat = probe_grid_centers(
    frame.origin_x, frame.origin_y,
    grid.neighbor_center_distance_m, grid.width, grid.height,
  );
  const n = grid.width * grid.height;
  const xs = new Float64Array(n);
  const ys = new Float64Array(n);
  const qs = new Int32Array(n);
  const rs = new Int32Array(n);
  let i = 0;
  for (let row = 0; row < grid.height; row++) {
    for (let col = 0; col < grid.width; col++) {
      const q = col - ((row - (row & 1)) / 2 | 0);
      xs[i] = flat[i * 2];
      ys[i] = flat[i * 2 + 1];
      qs[i] = q;
      rs[i] = row;
      i += 1;
    }
  }
  state.centerCache = { key, xs, ys, qs, rs, n, heights: null };
  state.heightRev = -1;
  invalidateMapCache();
  syncHeightGrid();
  return state.centerCache;
};

export const expandDirtyWorld = (wx, wy, padM) => {
  state.dirtyRect = expandDirtyAabb(state.dirtyRect, wx, wy, padM);
};

export const visibleCells = (padPx, { useDirty = false } = {}) => {
  const cache = ensureCenterCache();
  if (!cache) return new Int32Array(0);
  // useDirty with no rect = paint nothing (not the whole map).
  if (useDirty && !state.dirtyRect) return new Int32Array(0);
  const canvas = getCanvas();
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  const pad = padPx || 0;
  const z = state.camera.zoom;
  const halfW = w / 2;
  const halfH = h / 2;
  const cx = state.camera.cx;
  const cy = state.camera.cy;
  const dirty = useDirty ? state.dirtyRect : null;
  const out = [];
  for (let i = 0; i < cache.n; i++) {
    const x = cache.xs[i];
    const y = cache.ys[i];
    if (!cellInDirty(x, y, dirty)) continue;
    const sx = (x - cx) * z + halfW;
    const sy = (y - cy) * z + halfH;
    if (!cellInViewport(sx, sy, w, h, pad)) continue;
    out.push(i);
  }
  return Int32Array.from(out);
};

const hexCorners = (cx, cy, size) => {
  const pts = [];
  for (let i = 0; i < 6; i++) {
    const angle = (Math.PI / 180) * (60 * i - 30);
    pts.push([cx + size * Math.cos(angle), cy + size * Math.sin(angle)]);
  }
  return pts;
};

const drawHex = (targetCtx, cx, cy, size, fill, stroke, lineWidth = 1) => {
  const pts = hexCorners(cx, cy, size);
  targetCtx.beginPath();
  pts.forEach(([x, y], i) => (i === 0 ? targetCtx.moveTo(x, y) : targetCtx.lineTo(x, y)));
  targetCtx.closePath();
  if (fill) {
    targetCtx.fillStyle = fill;
    targetCtx.fill();
  }
  if (stroke) {
    targetCtx.strokeStyle = stroke;
    targetCtx.lineWidth = lineWidth;
    targetCtx.stroke();
  }
};

export const paintCellsTo = (targetCtx, indices, layer, showGrid, drawSize, sizePx, gridStroke, gridWidth) => {
  const cache = state.centerCache;
  if (!cache) return;
  const canvas = getCanvas();
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  const z = state.camera.zoom;
  const halfW = w / 2;
  const halfH = h / 2;
  const cx = state.camera.cx;
  const cy = state.camera.cy;
  const heights = cache.heights;
  const emptyFill = "#121821";
  for (let k = 0; k < indices.length; k++) {
    const i = indices[k];
    const sx = (cache.xs[i] - cx) * z + halfW;
    const sy = (cache.ys[i] - cy) * z + halfH;
    const cellH = heights ? heights[i] : 0;
    if (layer === "relief") {
      const rgb = reliefTintRgb(cellH);
      drawHex(targetCtx, sx, sy, drawSize, `rgb(${rgb[0]},${rgb[1]},${rgb[2]})`, gridStroke, gridWidth);
    } else {
      drawHex(targetCtx, sx, sy, drawSize, emptyFill, gridStroke, gridWidth);
    }
  }
};

const drawBrushOverlay = (drawSize, sizePx) => {
  if (state.activeTool !== "relief" || !state.hoverBrush || !state.spatial) return;
  const { frame, grid } = state.spatial.state;
  const footprint = diskCellsAt(
    state.hoverBrush.q, state.hoverBrush.r, state.brushRadius, grid.width, grid.height,
  );
  const ctx = getCtx();
  for (const cell of footprint) {
    const p = probe_axial_to_world(
      frame.origin_x, frame.origin_y,
      grid.neighbor_center_distance_m, grid.width, grid.height, cell.q, cell.r,
    );
    const [sx, sy] = worldToScreen(p.x, p.y);
    drawHex(ctx, sx, sy, drawSize, null, "rgba(232, 197, 71, 0.95)", Math.max(1.5, sizePx * 0.08));
  }
};

const mapStaticKey = (layer, showGrid, cssW, cssH) => {
  const rev = state.spatial?.state?.revision ?? 0;
  return `${rev}|${layer}|${showGrid ? 1 : 0}|${cssW}x${cssH}`;
};

const rebuildOffscreenCache = (layer, showGrid, w, h, drawSize, sizePx, gridStroke, gridWidth, padPx) => {
  ensureCenterCache();
  syncHeightGrid();
  const fullIdx = visibleCells(padPx, { useDirty: false });
  const canvas = getCanvas();
  const oc = document.createElement("canvas");
  oc.width = canvas.width;
  oc.height = canvas.height;
  const octx = oc.getContext("2d");
  const dpr = window.devicePixelRatio || 1;
  octx.setTransform(dpr, 0, 0, dpr, 0, 0);
  octx.fillStyle = "#0b0e13";
  octx.fillRect(0, 0, w, h);
  paintCellsTo(octx, fullIdx, layer, showGrid, drawSize, sizePx, gridStroke, gridWidth);
  state.offscreenCache = {
    canvas: oc,
    staticKey: mapStaticKey(layer, showGrid, w, h),
    camCx: state.camera.cx,
    camCy: state.camera.cy,
    zoom: state.camera.zoom,
    cssW: w,
    cssH: h,
  };
  return oc;
};

const scheduleCacheRebuild = () => {
  if (state.cacheRebuildRaf || state.paintStroke) return;
  state.cacheRebuildRaf = requestAnimationFrame(() => {
    state.cacheRebuildRaf = 0;
    if (state.paintStroke) return;
    state.offscreenCache = null;
    drawSpatialNow();
  });
};

export const drawSpatialNow = () => {
  const t0 = performance.now();
  const canvas = getCanvas();
  const ctx = getCtx();
  resizeCanvas();
  const w = canvas.clientWidth;
  const h = canvas.clientHeight;
  ctx.clearRect(0, 0, w, h);
  ctx.fillStyle = "#0b0e13";
  ctx.fillRect(0, 0, w, h);
  if (!state.spatial) {
    ctx.fillStyle = "#97a3b4";
    ctx.fillText("Open a world to load the map", 24, 32);
    state.lastFrameMs = performance.now() - t0;
    return;
  }
  const layer = state.mapLayer;
  const showGrid = state.viewHexGrid;
  const viewHelp = document.querySelector("#view-help");
  if (viewHelp) {
    viewHelp.textContent = layer === "relief"
      ? (showGrid
          ? "relief: elevation tint + outline overlay. Outlines are display-only."
          : "relief: elevation tint (no outline). Cells stay flush for metric scale.")
      : (showGrid
          ? "Empty: outline overlay on flush cells. Toggle Grid off for fill-only."
          : "Empty: flush cells, no outline. Toggle Grid on to see edges.");
  }
  const { grid } = state.spatial.state;
  const radiusM = grid.neighbor_center_distance_m / Math.sqrt(3);
  const sizePx = radiusM * state.camera.zoom;
  const drawSize = showGrid ? sizePx : sizePx + 0.9;
  const gridStroke = showGrid
    ? (layer === "empty" ? "rgba(140, 160, 190, 0.45)" : "rgba(20, 28, 36, 0.40)")
    : null;
  const gridWidth = showGrid ? 1.5 : 0;
  const padPx = Math.max(sizePx * 2, 24) + state.brushRadius * sizePx * 2;

  const painting = !!state.paintStroke;
  const staticKey = mapStaticKey(layer, showGrid, w, h);
  const cacheOk = state.offscreenCache && state.offscreenCache.staticKey === staticKey;
  const zoomMatch = cacheOk && Math.abs(state.offscreenCache.zoom - state.camera.zoom) < 1e-12;

  if (!painting && cacheOk && zoomMatch && !state.dirtyRect) {
    const dx = (state.offscreenCache.camCx - state.camera.cx) * state.camera.zoom;
    const dy = (state.offscreenCache.camCy - state.camera.cy) * state.camera.zoom;
    blitOffscreen(ctx, state.offscreenCache.canvas, dx, dy, w, h);
  } else if (!painting && cacheOk && !zoomMatch && !state.dirtyRect) {
    const scale = state.camera.zoom / state.offscreenCache.zoom;
    const dx = (state.offscreenCache.camCx - state.camera.cx) * state.camera.zoom;
    const dy = (state.offscreenCache.camCy - state.camera.cy) * state.camera.zoom;
    ctx.save();
    ctx.translate(w / 2 + dx, h / 2 + dy);
    ctx.scale(scale, scale);
    ctx.translate(-w / 2, -h / 2);
    blitOffscreen(ctx, state.offscreenCache.canvas, 0, 0, w, h);
    ctx.restore();
    scheduleCacheRebuild();
  } else if (painting && cacheOk && zoomMatch) {
    const dx = (state.offscreenCache.camCx - state.camera.cx) * state.camera.zoom;
    const dy = (state.offscreenCache.camCy - state.camera.cy) * state.camera.zoom;
    blitOffscreen(ctx, state.offscreenCache.canvas, dx, dy, w, h);
    // Keep stroke dirty until commit rebuild — clearing forced full-map patch flicker.
    if (state.dirtyRect) {
      syncHeightGrid();
      const patch = visibleCells(padPx, { useDirty: true });
      paintCellsTo(ctx, patch, layer, showGrid, drawSize, sizePx, gridStroke, gridWidth);
    }
  } else {
    const oc = rebuildOffscreenCache(layer, showGrid, w, h, drawSize, sizePx, gridStroke, gridWidth, padPx);
    blitOffscreen(ctx, oc, 0, 0, w, h);
    state.dirtyRect = null;
  }

  drawBrushOverlay(drawSize, sizePx);
  state.lastFrameMs = performance.now() - t0;
  if (!painting) syncViewCameraDebug();
};

export const drawSpatial = () => {
  if (state.drawRaf) return;
  state.drawRaf = requestAnimationFrame(() => {
    state.drawRaf = 0;
    drawSpatialNow();
  });
};

export const flushDrawSpatial = () => {
  if (state.drawRaf) {
    cancelAnimationFrame(state.drawRaf);
    state.drawRaf = 0;
  }
  drawSpatialNow();
};
