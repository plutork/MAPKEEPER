export const ELEV_MIN = -60;
export const ELEV_MAX = 100;
export const FIELD_FLUSH_BATCH_MAX = 512;

/** Sole owner of mutable Editor/view state (N-027). DOM is view only. */
export const state = {
  defaultRoot: "",
  folderTouched: false,
  pendingDeletePath: "",
  pendingDeleteId: "",
  spatial: null,
  camera: { zoom: 36, cx: 0, cy: 0 },
  // N-029: camera follows fit until the author sets a view; then resize keeps it.
  cameraFollowsFit: true,
  activeTool: "view",
  workspaceMode: "editor",
  mapLayer: "empty",
  viewHexGrid: true,
  reliefDirection: 1,
  /** raise | lower | flatten | smooth (N-038) */
  reliefOp: "raise",
  editOcean: false,
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
  zoomRebuildTimer: 0,
  lastFrameMs: 0,
  heightRev: -1,
};

export const axialKey = (q, r) => `${q},${r}`;
export const radiusStepSize = (r) => (r < 5 ? 1 : r < 12 ? 2 : 4);
export const diskCountEstimate = (r) => 3 * r * (r + 1) + 1;

export const setMapLayer = (layer) => {
  state.mapLayer = layer === "relief" ? "relief" : "empty";
};

export const setViewHexGrid = (on) => {
  state.viewHexGrid = !!on;
};

export const setReliefDirection = (dir) => {
  state.reliefDirection = dir < 0 ? -1 : 1;
  state.reliefOp = state.reliefDirection < 0 ? "lower" : "raise";
};

export const setReliefOp = (op) => {
  const allowed = ["raise", "lower", "flatten", "smooth"];
  state.reliefOp = allowed.includes(op) ? op : "raise";
  if (state.reliefOp === "raise") state.reliefDirection = 1;
  if (state.reliefOp === "lower") state.reliefDirection = -1;
};

export const setEditOcean = (on) => {
  state.editOcean = !!on;
};

/** N-029: deliberate pan/zoom detaches the camera from automatic fit. */
export const markCameraAuthorSet = () => {
  state.cameraFollowsFit = false;
};

/** N-029: open world / Reset zoom put the camera back under automatic fit. */
export const markCameraFollowsFit = () => {
  state.cameraFollowsFit = true;
};

/** Push listed Editor flags to DOM controls (view sync). */
export const syncEditorViewDom = () => {
  document.querySelectorAll("#map-layer-stack button").forEach((btn) => {
    btn.classList.toggle("active", btn.dataset.layer === state.mapLayer);
  });
  const gridBtn = document.querySelector("#view-hex-grid");
  if (gridBtn) {
    gridBtn.classList.toggle("active", state.viewHexGrid);
    gridBtn.setAttribute("aria-pressed", state.viewHexGrid ? "true" : "false");
  }
  document.querySelectorAll('input[name="relief-mode"]').forEach((input) => {
    input.checked = input.value === state.reliefOp;
  });
  const ocean = document.querySelector("#edit-ocean");
  if (ocean) {
    ocean.classList.toggle("active", state.editOcean);
    ocean.setAttribute("aria-pressed", state.editOcean ? "true" : "false");
  }
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

/** N-039: optimistic cells/heights are already current; advance only durable revision. */
export const applyStrokeAck = (ack) => {
  if (!state.spatial?.state || !Number.isInteger(ack?.revision)) {
    throw new Error("invalid stroke ACK");
  }
  state.spatial.state.revision = ack.revision;
  state.heightRev = state.centerCache?.heights ? ack.revision : -1;
};
