import { state } from "./workspace-state.js";
import { probe_grid_center_bounds } from "./wasm-api.js";
import {
  screenFromWorld, worldFromScreen, fitZoomForViewport,
} from "./shell-math.js";

let canvas, ctx;

/** Last fit diagnostics for View readout (dogfood). */
export let lastFitDebug = null;

export { fitZoomForViewport };

export const initCamera = (canvasEl, ctxRef) => {
  canvas = canvasEl;
  ctx = ctxRef;
};

export const resizeCanvas = () => {
  const host = canvas.parentElement;
  const rect = host.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  // Use visible host box (not content-sized overflow).
  const cssW = Math.max(1, rect.width);
  const cssH = Math.max(1, rect.height);
  const nextW = Math.max(320, Math.floor(cssW * dpr));
  const nextH = Math.max(240, Math.floor(cssH * dpr));
  // Same-size assign still clears the bitmap — skip when unchanged.
  if (canvas.width !== nextW || canvas.height !== nextH) {
    canvas.width = nextW;
    canvas.height = nextH;
  }
  canvas.style.width = `${cssW}px`;
  canvas.style.height = `${cssH}px`;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  return { cssW, cssH, dpr, hostH: rect.height, winH: window.innerHeight };
};

export const worldToScreen = (x, y) =>
  screenFromWorld(x, y, state.camera, canvas.clientWidth, canvas.clientHeight);

export const screenToWorld = (sx, sy) =>
  worldFromScreen(sx, sy, state.camera, canvas.clientWidth, canvas.clientHeight);

const fmt = (n, d = 4) => (Number.isFinite(n) ? n.toFixed(d) : "NaN");

/** Push camera/fit numbers into View panel for dogfood debug. */
export const syncViewCameraDebug = () => {
  const el = document.querySelector("#view-camera-debug");
  if (!el || !canvas) return;
  if (!state.spatial) {
    el.textContent = "Camera: (no spatial)";
    return;
  }
  const host = canvas.parentElement;
  const hostRect = host ? host.getBoundingClientRect() : { width: 0, height: 0 };
  const cssW = canvas.clientWidth;
  const cssH = canvas.clientHeight;
  const dpr = window.devicePixelRatio || 1;
  const { zoom, cx, cy } = state.camera;
  const { grid } = state.spatial.state;
  const neighborPx = zoom * (grid.neighbor_center_distance_m || 0);
  const layoutNote = hostRect.height > window.innerHeight * 0.95
    ? ""
    : (document.body.scrollHeight > window.innerHeight + 2 ? " · BODY_SCROLL" : "");
  let edge = "";
  if (lastFitDebug) {
    const [sx0] = screenFromWorld(lastFitDebug.worldMinX, lastFitDebug.worldMinY, state.camera, cssW, cssH);
    const [sx1] = screenFromWorld(lastFitDebug.worldMaxX, lastFitDebug.worldMaxY, state.camera, cssW, cssH);
    const [, sy0] = screenFromWorld(lastFitDebug.worldMinX, lastFitDebug.worldMinY, state.camera, cssW, cssH);
    const [, sy1] = screenFromWorld(lastFitDebug.worldMaxX, lastFitDebug.worldMaxY, state.camera, cssW, cssH);
    const cropX = sx0 < -1 || sx1 > cssW + 1;
    const cropY = Math.min(sy0, sy1) < -1 || Math.max(sy0, sy1) > cssH + 1;
    edge =
      ` · edgeX=${fmt(sx0, 1)}…${fmt(sx1, 1)}/${fmt(cssW, 0)}${cropX ? " CROPPED" : " ok"}` +
      ` · edgeY=${fmt(Math.min(sy0, sy1), 1)}…${fmt(Math.max(sy0, sy1), 1)}/${fmt(cssH, 0)}${cropY ? " CROPPED" : " ok"}`;
    el.textContent =
      `Camera: z=${fmt(zoom, 5)} cx=${fmt(cx, 0)} cy=${fmt(cy, 0)} · ` +
      `css=${fmt(cssW, 0)}×${fmt(cssH, 0)} host=${fmt(hostRect.height, 0)} win=${fmt(window.innerHeight, 0)} dpr=${fmt(dpr, 2)} · ` +
      `span=${fmt(lastFitDebug.spanX, 0)}×${fmt(lastFitDebug.spanY, 0)}m · ` +
      `zX=${fmt(lastFitDebug.zoomX, 5)} zY=${fmt(lastFitDebug.zoomY, 5)} ` +
      `limit=${lastFitDebug.limit} · nPx=${fmt(neighborPx, 1)} · grid=${grid.width}×${grid.height}` +
      edge + layoutNote;
    return;
  }
  el.textContent =
    `Camera: z=${fmt(zoom, 5)} cx=${fmt(cx, 0)} cy=${fmt(cy, 0)} · ` +
    `css=${fmt(cssW, 0)}×${fmt(cssH, 0)} host=${fmt(hostRect.height, 0)} win=${fmt(window.innerHeight, 0)} · ` +
    `nPx=${fmt(neighborPx, 1)} · grid=${grid.width}×${grid.height} · (no fit yet)` + layoutNote;
};

export const fitCamera = () => {
  if (!state.spatial || !canvas) return;
  // Fit must use laid-out CSS size (Home→Workspace open often races layout).
  const layout = resizeCanvas();
  const { frame, grid } = state.spatial.state;
  const bounds = probe_grid_center_bounds(
    frame.origin_x, frame.origin_y,
    grid.neighbor_center_distance_m, grid.width, grid.height,
  );
  const minX = bounds[0];
  const minY = bounds[1];
  const maxX = bounds[2];
  const maxY = bounds[3];
  state.camera.cx = (minX + maxX) / 2;
  state.camera.cy = (minY + maxY) / 2;
  // Match catalog metric_extent: vertex pad = red-blob size on both axes.
  const n = grid.neighbor_center_distance_m;
  const size = n / Math.sqrt(3);
  const worldMinX = minX - size;
  const worldMaxX = maxX + size;
  const worldMinY = minY - size;
  const worldMaxY = maxY + size;
  const cssW = layout.cssW;
  const cssH = layout.cssH;
  const spanX = Math.max(n, worldMaxX - worldMinX);
  const spanY = Math.max(n, worldMaxY - worldMinY);
  const fit = fitZoomForViewport(cssW, cssH, spanX, spanY);
  state.camera.zoom = fit.zoom;
  lastFitDebug = {
    spanX, spanY, zoomX: cssW / spanX, zoomY: cssH / spanY,
    limit: fit.limit, cssW, cssH,
    worldMinX, worldMaxX, worldMinY, worldMaxY,
    centerMinX: minX, centerMaxX: maxX, centerMinY: minY, centerMaxY: maxY,
    hostH: layout.hostH, winH: layout.winH,
    containZ: fit.containZ,
  };
  syncViewCameraDebug();
};

let hostObserver = null;
let lastHostBox = null;

/**
 * N-029: react to canvas host size from any source (window, side panels, mode
 * switch). Only a real host box change fires, so a fit that rewrites the canvas
 * box cannot re-enter this callback.
 */
export const observeCanvasHost = (onHostResize) => {
  const host = canvas?.parentElement;
  if (!host) return;
  if (typeof ResizeObserver === "undefined") {
    // Older webview: window resize only, panels/mode switches go unnoticed.
    window.addEventListener("resize", onHostResize);
    return;
  }
  hostObserver?.disconnect();
  const rect = host.getBoundingClientRect();
  lastHostBox = { w: Math.round(rect.width), h: Math.round(rect.height) };
  hostObserver = new ResizeObserver(() => {
    const box = host.getBoundingClientRect();
    const w = Math.round(box.width);
    const h = Math.round(box.height);
    if (lastHostBox && w === lastHostBox.w && h === lastHostBox.h) return;
    lastHostBox = { w, h };
    onHostResize();
  });
  hostObserver.observe(host);
};

export const getCanvas = () => canvas;
export const getCtx = () => ctx;
