/**
 * N-026 maintainer bench suite — loaded only in explicit bench mode (?bench=1).
 */
import { state, setMapLayer } from "./workspace-state.js";
import { fitCamera } from "./camera.js";
import {
  flushDrawSpatial, invalidateMapCache, ensureCenterCache, visibleCells,
} from "./renderer.js";
import {
  beginPaintStroke, extendPaintStroke, beginAirbrushEpoch, clearAirbrushTimer,
} from "./relief-tool.js";

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

export const installBenchHooks = () => {
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
};
