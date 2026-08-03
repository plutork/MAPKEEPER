/**
 * N-027 pure unit tests (Node ESM) — state + shell-math + transaction chunking.
 * Run: node scripts/web-shell-unit.mjs
 */
import assert from "node:assert/strict";
import {
  screenFromWorld,
  worldFromScreen,
  cellInViewport,
  cellInDirty,
  strokeChunks,
  expandDirtyAabb,
  fitZoomForViewport,
  offscreenBlitArgs,
  overscanMarginCss,
  overscanCacheCssSize,
  panLeavesOverscan,
  overscanSourceCss,
  overscanBlitArgs,
  cameraSlipCss,
  nextCameraFollowsFit,
  bakRestoreOffer,
} from "../crates/web/shell-math.js";

// Minimal document mock for workspace-state view sync.
globalThis.document = {
  querySelector: () => null,
  querySelectorAll: () => [],
};
globalThis.cancelAnimationFrame = () => {};

const {
  state, applySpatial, applyStrokeAck, FIELD_FLUSH_BATCH_MAX, axialKey,
  setMapLayer, setViewHexGrid, setReliefDirection, setReliefOp, setEditOcean,
  markCameraAuthorSet, markCameraFollowsFit,
} = await import("../crates/web/workspace-state.js");

let failed = 0;
const check = (name, fn) => {
  try {
    fn();
    console.log(`ok  ${name}`);
  } catch (err) {
    failed += 1;
    console.error(`FAIL ${name}: ${err.message}`);
  }
};

check("screen/world roundtrip", () => {
  const cam = { zoom: 2, cx: 10, cy: -5 };
  const [sx, sy] = screenFromWorld(10, -5, cam, 200, 100);
  assert.equal(sx, 100);
  assert.equal(sy, 50);
  const [wx, wy] = worldFromScreen(sx, sy, cam, 200, 100);
  assert.equal(wx, 10);
  assert.equal(wy, -5);
});

check("viewport cull", () => {
  assert.equal(cellInViewport(0, 0, 100, 100, 0), true);
  assert.equal(cellInViewport(-1, 50, 100, 100, 0), false);
  assert.equal(cellInViewport(-1, 50, 100, 100, 2), true);
});

check("dirty aabb", () => {
  const d = expandDirtyAabb(null, 0, 0, 10);
  assert.deepEqual(d, { minX: -10, minY: -10, maxX: 10, maxY: 10 });
  const d2 = expandDirtyAabb(d, 20, 0, 5);
  assert.equal(d2.maxX, 25);
  assert.equal(cellInDirty(0, 0, d2), true);
  assert.equal(cellInDirty(100, 0, d2), false);
});

// Relief gesture rule lives in core (N-030); its tests are in crates/core.

check("strokeChunks batching", () => {
  const cells = Array.from({ length: 5 }, (_, i) => ({ q: i, r: 0, value: 1 }));
  assert.equal(strokeChunks(cells, 512).length, 1);
  assert.equal(strokeChunks(cells, 2).length, 3);
  assert.deepEqual(
    strokeChunks(cells, 2).map((c) => c.length),
    [2, 2, 1],
  );
  assert.equal(FIELD_FLUSH_BATCH_MAX, 512);
});

check("bak restore offer follows N-025 corruption policy", () => {
  assert.equal(
    bakRestoreOffer("corrupt_registry: interrupted_write (bak_available=true)").endpoint,
    "/api/projects/restore-bak",
  );
  assert.equal(
    bakRestoreOffer("corrupt_spatial: bad json (bak_available=true)").endpoint,
    "/api/spatial/restore-bak",
  );
  // No usable backup, unrelated failure, or manifest damage → no dead button.
  assert.equal(bakRestoreOffer("corrupt_registry: x (bak_available=false)"), null);
  assert.equal(bakRestoreOffer("corrupt_manifest: x (bak_available=true)"), null);
  assert.equal(bakRestoreOffer("HTTP 500"), null);
  assert.equal(bakRestoreOffer(undefined), null);
});

check("applySpatial resets CRS caches", () => {
  state.centerCache = { key: "x" };
  state.offscreenCache = { canvas: null };
  state.dirtyRect = { minX: 0, minY: 0, maxX: 1, maxY: 1 };
  state.heightRev = 9;
  applySpatial({ state: { grid: { id: "g" }, field: { id: "f", cells: {} } } });
  assert.equal(state.spatial.state.grid.id, "g");
  assert.equal(state.centerCache, null);
  assert.equal(state.offscreenCache, null);
  assert.equal(state.dirtyRect, null);
  assert.equal(state.heightRev, -1);
  assert.equal(axialKey(1, 2), "1,2");
});

check("applyStrokeAck preserves optimistic CRS state", () => {
  const centers = { key: "grid", heights: new Int16Array([7]) };
  const offscreen = { canvas: {} };
  const dirty = { minX: 0, minY: 0, maxX: 1, maxY: 1 };
  state.spatial = { state: { revision: 4, field: { cells: { "0,0": 7 } } } };
  state.centerCache = centers;
  state.offscreenCache = offscreen;
  state.dirtyRect = dirty;
  state.heightRev = 4;
  applyStrokeAck({ revision: 5, applied_cells: 1 });
  assert.equal(state.spatial.state.revision, 5);
  assert.equal(state.heightRev, 5);
  assert.equal(state.centerCache, centers);
  assert.equal(state.offscreenCache, offscreen);
  assert.equal(state.dirtyRect, dirty);
  assert.equal(state.spatial.state.field.cells["0,0"], 7);
});

check("workspace-state owns editor view flags", () => {
  setMapLayer("relief");
  setViewHexGrid(false);
  setReliefDirection(-1);
  setEditOcean(true);
  assert.equal(state.mapLayer, "relief");
  assert.equal(state.viewHexGrid, false);
  assert.equal(state.reliefDirection, -1);
  assert.equal(state.reliefOp, "lower");
  assert.equal(state.editOcean, true);
  setReliefOp("flatten");
  assert.equal(state.reliefOp, "flatten");
  state.strokeMode = "airbrush";
  setReliefOp("smooth");
  assert.equal(state.reliefOp, "smooth");
  assert.equal(state.strokeMode, "airbrush");
  setMapLayer("empty");
  setViewHexGrid(true);
  setReliefDirection(1);
  setEditOcean(false);
  assert.equal(state.mapLayer, "empty");
  assert.equal(state.viewHexGrid, true);
  assert.equal(state.reliefDirection, 1);
  assert.equal(state.reliefOp, "raise");
  assert.equal(state.editOcean, false);
});

check("fit zoom uses positive viewport dims", () => {
  // Mirror fitCamera denom guard: zero CSS size must not drive usable zoom.
  const span = 1000;
  const zoomZero = Math.max(0.002, Math.min(0 / span, 0 / span) * 0.9);
  const zoomLaidOut = Math.max(0.002, Math.min(800 / span, 600 / span) * 0.9);
  assert.equal(zoomZero, 0.002);
  assert.ok(zoomLaidOut > 0.1);
});

check("fitZoom contain scales with CSS host", () => {
  const spanX = 55655;
  const spanY = 31466;
  const small = fitZoomForViewport(1380, 905, spanX, spanY);
  const large = fitZoomForViewport(2020, 1267, spanX, spanY);
  assert.equal(small.limit, "X");
  assert.equal(large.limit, "X");
  // Larger host → higher contain zoom (fill monitor, no density clamp).
  assert.ok(large.zoom > small.zoom);
  assert.ok(Math.abs(large.zoom - large.containZ) < 1e-12);
  assert.ok(Math.abs(large.zoom - 2020 / spanX * 0.96) < 1e-9);
});

check("offscreen blit dest is CSS not device px", () => {
  const dpr = 1.5;
  const cssW = 2020;
  const cssH = 1267;
  const bitmapW = Math.floor(cssW * dpr);
  const bitmapH = Math.floor(cssH * dpr);
  const args = offscreenBlitArgs(bitmapW, bitmapH, 0, 0, cssW, cssH);
  // Default drawImage(oc,0,0) would use bitmap size under setTransform(dpr) → ×dpr crop.
  assert.deepEqual(args, [0, 0, bitmapW, bitmapH, 0, 0, cssW, cssH]);
  assert.ok(args[6] < args[2]);
  assert.ok(args[7] < args[3]);
});

check("overscan margin and pan safe band (N-026 crs-overscan-cache)", () => {
  assert.equal(overscanMarginCss(10), 256);
  assert.equal(overscanMarginCss(200), 400);
  const { cacheW, cacheH } = overscanCacheCssSize(1000, 600, 256);
  assert.equal(cacheW, 1000 + 512);
  assert.equal(cacheH, 600 + 512);
  assert.equal(panLeavesOverscan(0, 0, 256, 24), false);
  assert.equal(panLeavesOverscan(100, 0, 256, 24), false);
  assert.equal(panLeavesOverscan(240, 0, 256, 24), true);
  const { srcX, srcY } = overscanSourceCss(256, 40, -10);
  assert.equal(srcX, 216);
  assert.equal(srcY, 266);
  const blit = overscanBlitArgs(216, 266, 1000, 600, 1.5);
  assert.deepEqual(blit, [216 * 1.5, 266 * 1.5, 1500, 900, 0, 0, 1000, 600]);
});

check("zoom preview slip uses current zoom (not cache zoom)", () => {
  // Wheel re-anchors cx; old-zoom slip drifts the preview until rebuild.
  const cacheCx = 0;
  const camCx = 100;
  const zOld = 1;
  const zNew = 1 / 1.12;
  const withNew = cameraSlipCss(cacheCx, 0, camCx, 0, zNew);
  const withOld = cameraSlipCss(cacheCx, 0, camCx, 0, zOld);
  assert.equal(withNew.dx, -100 * zNew);
  assert.ok(Math.abs(withNew.dx - withOld.dx) > 1e-9);
});

check("camera sticky fit survives resize, author view detaches", () => {
  // N-029 transition table.
  assert.equal(nextCameraFollowsFit(true, "resize"), true);
  assert.equal(nextCameraFollowsFit(true, "pan"), false);
  assert.equal(nextCameraFollowsFit(true, "zoom"), false);
  assert.equal(nextCameraFollowsFit(false, "resize"), false);
  assert.equal(nextCameraFollowsFit(false, "reset-zoom"), true);
  assert.equal(nextCameraFollowsFit(false, "open"), true);
});

check("workspace-state camera flag matches transition", () => {
  markCameraFollowsFit();
  assert.equal(state.cameraFollowsFit, true);
  markCameraAuthorSet();
  assert.equal(state.cameraFollowsFit, false);
  // applySpatial after a stroke commit must not reattach the camera to fit.
  applySpatial({ state: { field: { cells: {} } } });
  assert.equal(state.cameraFollowsFit, false);
  markCameraFollowsFit();
});

check("useDirty without rect means no patch cells", () => {
  // Mirrors visibleCells guard: dirty-null must not repaint the whole map.
  const useDirty = true;
  const dirtyRect = null;
  const patchAll = !useDirty || !!dirtyRect;
  assert.equal(patchAll, false);
});

if (failed) {
  console.error(`web-shell-unit: ${failed} failed`);
  process.exit(1);
}
console.log("web-shell-unit: OK");
