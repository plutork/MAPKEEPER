export const ELEV_MIN = -60;
export const ELEV_MAX = 100;
export const FIELD_FLUSH_BATCH_MAX = 512;

export const state = {
  defaultRoot: "",
  folderTouched: false,
  pendingDeletePath: "",
  pendingDeleteId: "",
  spatial: null,
  camera: { zoom: 36, cx: 0, cy: 0 },
  activeTool: "view",
  workspaceMode: "editor",
  panDrag: null,
  brushRadius: 0,
  brushMaxRadius: 24,
  strokeMode: "stamp",
  airbrushRate: 5,
  hoverBrush: null,
  paintStroke: null,
  // CRS cache fields
  centerCache: null,
  drawRaf: 0,
  offscreenCache: null,
  dirtyRect: null,
  cacheRebuildRaf: 0,
  lastFrameMs: 0,
  perfSamples: [],
  heightRev: -1,
};

export const axialKey = (q, r) => `${q},${r}`;
export const radiusStepSize = (r) => (r < 5 ? 1 : r < 12 ? 2 : 4);
export const diskCountEstimate = (r) => 3 * r * (r + 1) + 1;

export const reliefMode = () => {
  const selected = document.querySelector('input[name="relief-mode"]:checked');
  return selected && selected.value === "lower" ? -1 : 1;
};

export const editOceanOn = () => {
  const el = document.querySelector("#edit-ocean");
  return el ? el.classList.contains("active") : false;
};

export const viewHexGridOn = () => {
  const el = document.querySelector("#view-hex-grid");
  return el ? el.classList.contains("active") : true;
};

export const mapLayer = () => {
  const selected = document.querySelector("#map-layer-stack button.active");
  return selected && selected.dataset.layer === "relief" ? "relief" : "empty";
};

export const setMapLayer = (layer) => {
  document.querySelectorAll("#map-layer-stack button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.layer === layer);
  });
};

export const applySpatial = (view) => {
  state.spatial = view;
  state.centerCache = null;
  state.heightRev = -1;
  state.offscreenCache = null;
  state.dirtyRect = null;
  if (state.cacheRebuildRaf) {
    cancelAnimationFrame(state.cacheRebuildRaf);
    state.cacheRebuildRaf = 0;
  }
};
