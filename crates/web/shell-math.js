/** Pure shell helpers (N-027) — no DOM, no WASM. */

export const screenFromWorld = (x, y, camera, cssW, cssH) => [
  (x - camera.cx) * camera.zoom + cssW / 2,
  (y - camera.cy) * camera.zoom + cssH / 2,
];

export const worldFromScreen = (sx, sy, camera, cssW, cssH) => [
  (sx - cssW / 2) / camera.zoom + camera.cx,
  (sy - cssH / 2) / camera.zoom + camera.cy,
];

export const cellInViewport = (sx, sy, cssW, cssH, pad) =>
  !(sx < -pad || sy < -pad || sx > cssW + pad || sy > cssH + pad);

/** Center inset by hex radius → whole hex fits (legacy cull helper). */
export const cellFullyInViewport = (sx, sy, cssW, cssH, hexRadiusPx) => {
  const r = Math.max(0, hexRadiusPx);
  return sx >= r && sy >= r && sx <= cssW - r && sy <= cssH - r;
};

export const cellInDirty = (x, y, dirty) => {
  if (!dirty) return true;
  return !(x < dirty.minX || x > dirty.maxX || y < dirty.minY || y > dirty.maxY);
};

/** Split stroke cells into begin/chunk transport batches (N-025). */
export const strokeChunks = (cells, maxBatch) => {
  if (!Array.isArray(cells) || maxBatch < 1) return [];
  if (cells.length <= maxBatch) return [cells];
  const out = [];
  for (let i = 0; i < cells.length; i += maxBatch) {
    out.push(cells.slice(i, i + maxBatch));
  }
  return out;
};

/**
 * N-025 corruption policy: classify a durable-open failure into the recovery
 * the author may be offered. `null` when nothing can be restored.
 */
export const bakRestoreOffer = (message) => {
  const text = String(message ?? "");
  const endpoint = text.includes("corrupt_registry")
    ? "/api/projects/restore-bak"
    : text.includes("corrupt_spatial")
      ? "/api/spatial/restore-bak"
      : null;
  // corrupt_manifest has no restore route yet — never offer a dead action.
  if (!endpoint || !/bak_available=true/.test(text)) return null;
  return {
    endpoint,
    label: endpoint.includes("projects") ? "Restore world list" : "Restore last saved map",
  };
};

export const expandDirtyAabb = (dirty, wx, wy, padM) => {
  if (!dirty) {
    return { minX: wx - padM, minY: wy - padM, maxX: wx + padM, maxY: wy + padM };
  }
  return {
    minX: Math.min(dirty.minX, wx - padM),
    minY: Math.min(dirty.minY, wy - padM),
    maxX: Math.max(dirty.maxX, wx + padM),
    maxY: Math.max(dirty.maxY, wy + padM),
  };
};

/** Contain ×0.96 to current CSS host (open / Reset zoom). */
export const fitZoomForViewport = (cssW, cssH, spanX, spanY) => {
  const containZ = Math.min(cssW / spanX, cssH / spanY) * 0.96;
  const zoom = Math.max(0.002, containZ);
  return {
    zoom,
    containZ,
    limit: cssW / spanX <= cssH / spanY ? "X" : "Y",
  };
};

/**
 * N-029 camera state transition. `follows fit` survives resize; a deliberate
 * pan/zoom detaches it, open and Reset zoom reattach it.
 */
export const nextCameraFollowsFit = (followsFit, event) => {
  if (event === "open" || event === "reset-zoom") return true;
  if (event === "pan" || event === "zoom") return false;
  return followsFit;
};

/**
 * drawImage args for a HiDPI offscreen cache while CTM is setTransform(dpr).
 * Dest size must be CSS px — intrinsic bitmap size under dpr CTM over-scales.
 */
export const offscreenBlitArgs = (bitmapW, bitmapH, dx, dy, cssW, cssH) => (
  [0, 0, bitmapW, bitmapH, dx, dy, cssW, cssH]
);

/** N-026 overscan: margin around viewport (GATE crs-overscan-cache default A). */
export const overscanMarginCss = (sizePx) => Math.max(256, 2 * Math.max(0, sizePx));

export const overscanCacheCssSize = (viewW, viewH, margin) => ({
  cacheW: viewW + 2 * margin,
  cacheH: viewH + 2 * margin,
});

/** True when pan slip leaves the safe band (margin minus one hex radius). */
export const panLeavesOverscan = (dx, dy, margin, sizePx) => {
  const safe = Math.max(0, margin - Math.max(0, sizePx));
  return Math.abs(dx) > safe || Math.abs(dy) > safe;
};

/**
 * Source CSS origin inside overscan cache for current pan slip.
 * Cache was painted with viewport at (margin, margin).
 */
export const overscanSourceCss = (margin, dx, dy) => ({
  srcX: margin - dx,
  srcY: margin - dy,
});

/**
 * drawImage args: overscan bitmap → viewport (CTM = setTransform(dpr)).
 * Source rect in device px; dest in CSS px.
 */
export const overscanBlitArgs = (srcCssX, srcCssY, viewW, viewH, dpr) => {
  const s = Math.max(1e-6, dpr);
  return [srcCssX * s, srcCssY * s, viewW * s, viewH * s, 0, 0, viewW, viewH];
};

/** Pan slip in CSS px for cache→camera; must use *current* zoom (wheel re-anchors cx/cy). */
export const cameraSlipCss = (cacheCamCx, cacheCamCy, camCx, camCy, zoom) => ({
  dx: (cacheCamCx - camCx) * zoom,
  dy: (cacheCamCy - camCy) * zoom,
});
