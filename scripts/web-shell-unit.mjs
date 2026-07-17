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
  nextElevationValue,
  strokeChunks,
  expandDirtyAabb,
} from "../crates/web/shell-math.js";

// Minimal document mock for workspace-state DOM helpers.
globalThis.document = {
  querySelector: () => null,
  querySelectorAll: () => [],
};
globalThis.cancelAnimationFrame = () => {};

const { state, applySpatial, FIELD_FLUSH_BATCH_MAX, axialKey } = await import(
  "../crates/web/workspace-state.js"
);

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

check("nextElevation ocean freeze", () => {
  assert.equal(nextElevationValue(-2, -1, { editOcean: false, elevMin: -60, elevMax: 100 }), null);
  assert.equal(nextElevationValue(-2, -1, { editOcean: true, elevMin: -60, elevMax: 100 }), -3);
  assert.equal(nextElevationValue(1, -1, { editOcean: false, elevMin: -60, elevMax: 100 }), 0);
});

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

if (failed) {
  console.error(`web-shell-unit: ${failed} failed`);
  process.exit(1);
}
console.log("web-shell-unit: OK");
