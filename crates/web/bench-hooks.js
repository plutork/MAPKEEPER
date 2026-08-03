/**
 * N-026 maintainer bench suite — loaded only in explicit bench mode (?bench=1).
 */
import { state, setMapLayer, setReliefOp } from "./workspace-state.js";
import { fitCamera } from "./camera.js";
import {
  flushDrawSpatial, invalidateMapCache, ensureCenterCache, visibleCells,
} from "./renderer.js";
import {
  beginPaintStroke, extendPaintStroke, beginAirbrushEpoch, clearAirbrushTimer,
} from "./relief-tool.js";
import { endPaintStroke } from "./spatial-transaction.js";

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

const summarizeAuthoring = (samples) => {
  const values = (pick) => samples.map(pick).filter((v) => Number.isFinite(v));
  const p95 = (pick) => percentile(values(pick), 95);
  return {
    n: samples.length,
    p50: percentile(values((s) => s.total_ms), 50),
    p95: p95((s) => s.total_ms),
    max: Math.max(...values((s) => s.total_ms)),
    applied_cells_p50: percentile(values((s) => s.applied_cells), 50),
    phases_p95: {
      durable_ack_ms: p95((s) => s.durable_ack_ms),
      server_read_ms: p95((s) => s.server_timings?.read_ms),
      server_parse_ms: p95((s) => s.server_timings?.parse_ms),
      server_apply_ms: p95((s) => s.server_timings?.apply_ms),
      server_serialize_ms: p95((s) => s.server_timings?.serialize_ms),
      server_atomic_write_ms: p95((s) => s.server_timings?.atomic_write_ms),
      response_bytes: p95((s) => s.network?.response_bytes),
      client_parse_ms: p95((s) => s.network?.parse_ms),
      client_apply_ms: p95((s) => s.client_apply_ms),
      first_correct_frame_ms: p95((s) => s.first_correct_frame_ms),
    },
  };
};

const authoringCell = (index, radius) => {
  const { width, height } = state.spatial.state.grid;
  const margin = Math.min(radius + 1, Math.max(0, Math.floor(Math.min(width, height) / 4)));
  const innerW = Math.max(1, width - 2 * margin);
  const innerH = Math.max(1, height - 2 * margin);
  const slot = index % (innerW * innerH);
  const col = margin + (slot % innerW);
  const r = margin + Math.floor(slot / innerW);
  const q = col - ((r - (r & 1)) / 2 | 0);
  return { q, r };
};

const runAuthoringStroke = async (index, radius) => {
  state.activeTool = "relief";
  state.strokeMode = "stamp";
  state.brushRadius = radius;
  setReliefOp("raise");
  setMapLayer("relief");
  const { q, r } = authoringCell(index, radius);
  const beforeRevision = state.spatial.state.revision;
  beginPaintStroke(q, r);
  flushDrawSpatial();
  const centerCache = state.centerCache;
  const offscreenCache = state.offscreenCache;
  const mouseupStarted = performance.now();
  const trace = await endPaintStroke();
  if (trace?.error) throw new Error(trace.error);
  await new Promise((resolve) => requestAnimationFrame(resolve));
  await new Promise((resolve) => requestAnimationFrame(resolve));
  const totalMs = performance.now() - mouseupStarted;
  if (state.paintStroke || state.spatial.state.revision <= beforeRevision) {
    throw new Error("authoring stroke was not ready after durable ACK");
  }
  if (
    trace.response_kind === "delta_ack"
    && (
      state.centerCache !== centerCache
      || state.offscreenCache !== offscreenCache
      || state.dirtyRect
    )
  ) {
    throw new Error("delta ACK did not preserve and settle CRS caches");
  }
  const network = trace.phases?.network?.at(-1) ?? null;
  return {
    total_ms: totalMs,
    applied_cells: trace.applied_cells,
    response_kind: trace.response_kind,
    durable_ack_ms: trace.phases?.durable_ack_ms ?? null,
    client_apply_ms: trace.phases?.client_apply_ms ?? null,
    first_correct_frame_ms: Math.max(
      0,
      totalMs
        - (trace.phases?.durable_ack_ms ?? 0)
        - (trace.phases?.client_apply_ms ?? 0),
    ),
    server_timings: trace.server_timings,
    network,
  };
};

export const installBenchHooks = () => {
  window.__MK_BENCH__ = {
    lastFrameMs: () => state.lastFrameMs,
    flushDraw: () => flushDrawSpatial(),
    ensureCenters: () => ensureCenterCache(),
    visibleCount: () => visibleCells(64, { useDirty: false }).length,
    cellCount: () => (state.spatial ? state.spatial.state.grid.width * state.spatial.state.grid.height : 0),
    async runAuthoringSuite() {
      if (!state.spatial) throw new Error("no active world");
      // Let open's double-rAF fit finish before asserting cache identity.
      await new Promise((resolve) => requestAnimationFrame(resolve));
      await new Promise((resolve) => requestAnimationFrame(resolve));
      setMapLayer("relief");
      invalidateMapCache();
      flushDrawSpatial();
      await runAuthoringStroke(900, 0);
      const small = [];
      const medium = [];
      const series100 = [];
      for (let i = 0; i < 5; i++) small.push(await runAuthoringStroke(i, 0));
      for (let i = 0; i < 5; i++) medium.push(await runAuthoringStroke(100 + i * 17, 4));
      for (let i = 0; i < 100; i++) series100.push(await runAuthoringStroke(300 + i, 0));
      return {
        cell_count: this.cellCount(),
        final_revision: state.spatial.state.revision,
        budget_p95_ms: 100,
        small: summarizeAuthoring(small),
        medium: summarizeAuthoring(medium),
        series_100_small: summarizeAuthoring(series100),
      };
    },
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
