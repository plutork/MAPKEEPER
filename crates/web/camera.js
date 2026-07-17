import { state } from "./workspace-state.js";
import { probe_grid_center_bounds } from "./wasm-api.js";
import { screenFromWorld, worldFromScreen } from "./shell-math.js";

let canvas, ctx;

export const initCamera = (canvasEl, ctxRef) => {
  canvas = canvasEl;
  ctx = ctxRef;
};

export const resizeCanvas = () => {
  const rect = canvas.parentElement.getBoundingClientRect();
  const dpr = window.devicePixelRatio || 1;
  canvas.width = Math.max(320, Math.floor(rect.width * dpr));
  canvas.height = Math.max(240, Math.floor(rect.height * dpr));
  canvas.style.width = `${rect.width}px`;
  canvas.style.height = `${rect.height}px`;
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
};

export const worldToScreen = (x, y) =>
  screenFromWorld(x, y, state.camera, canvas.clientWidth, canvas.clientHeight);

export const screenToWorld = (sx, sy) =>
  worldFromScreen(sx, sy, state.camera, canvas.clientWidth, canvas.clientHeight);

export const fitCamera = () => {
  if (!state.spatial) return;
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
  const pad = 2.5 * grid.neighbor_center_distance_m;
  const zoomX = canvas.clientWidth / (maxX - minX + pad);
  const zoomY = canvas.clientHeight / (maxY - minY + pad);
  state.camera.zoom = Math.max(0.002, Math.min(zoomX, zoomY) * 0.9);
};

export const getCanvas = () => canvas;
export const getCtx = () => ctx;
