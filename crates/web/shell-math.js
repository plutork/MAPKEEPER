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

export const cellInDirty = (x, y, dirty) => {
  if (!dirty) return true;
  return !(x < dirty.minX || x > dirty.maxX || y < dirty.minY || y > dirty.maxY);
};

export const nextElevationValue = (current, delta, { editOcean, elevMin, elevMax }) => {
  if (current < 0 && !editOcean) return null;
  let next = current + delta;
  if (delta < 0 && !editOcean) next = Math.max(0, next);
  next = Math.max(elevMin, Math.min(elevMax, next));
  return next === current ? null : next;
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
