import {
  state, FIELD_FLUSH_BATCH_MAX, applySpatial, applyStrokeAck, markCameraFollowsFit,
} from "./workspace-state.js";
import { api, newStrokeId } from "./api.js";
import {
  ensureCenterCache, drawSpatial, invalidateMapCache, commitDirtyMapCache,
} from "./renderer.js";
import { fitCamera } from "./camera.js";
import { clearAirbrushTimer, refreshBrushMaxFromGrid, syncBrushRadiusUi } from "./relief-tool.js";
import { strokeChunks } from "./shell-math.js";

export const commitStrokeCells = async (cells, { onTiming } = {}) => {
  const baseRevision = state.spatial?.state?.revision ?? 0;
  const strokeId = newStrokeId();
  const chunks = strokeChunks(cells, FIELD_FLUSH_BATCH_MAX);
  if (chunks.length <= 1) {
    return api("/api/spatial/stroke", {
      method: "POST",
      body: JSON.stringify({
        stroke_id: strokeId,
        base_revision: baseRevision,
        cells,
      }),
      onTiming,
    });
  }
  await api("/api/spatial/stroke/begin", {
    method: "POST",
    body: JSON.stringify({
      stroke_id: strokeId,
      base_revision: baseRevision,
    }),
    onTiming,
  });
  try {
    for (let i = 0; i < chunks.length; i++) {
      await api("/api/spatial/stroke/chunk", {
        method: "POST",
        body: JSON.stringify({
          stroke_id: strokeId,
          chunk_id: String(i),
          cells: chunks[i],
        }),
        onTiming,
      });
    }
    return await api("/api/spatial/stroke/commit", {
      method: "POST",
      body: JSON.stringify({ stroke_id: strokeId }),
      onTiming,
    });
  } catch (error) {
    await api("/api/spatial/stroke/abort", {
      method: "POST",
      body: JSON.stringify({ stroke_id: strokeId }),
    }).catch(() => {});
    throw error;
  }
};

export const fullApplySpatial = (view, { refit = false } = {}) => {
  applySpatial(view);
  const spatialStatus = document.querySelector("#spatial-status");
  if (spatialStatus) {
    spatialStatus.textContent = `grid=${view.state.grid.id} · field=${view.state.field.id}`;
  }
  if (refit) {
    fitCamera();
  }
  ensureCenterCache();
  refreshBrushMaxFromGrid();
  syncBrushRadiusUi();
  drawSpatial();
};

export const loadSpatial = async () => {
  const view = await api("/api/spatial");
  // N-029: a freshly loaded map starts under automatic fit again.
  markCameraFollowsFit();
  fullApplySpatial(view, { refit: true });
  // Second fit after layout flush (workspace just revealed).
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      if (!state.spatial) return;
      fitCamera();
      ensureCenterCache();
      invalidateMapCache();
      drawSpatial();
    });
  });
};

export const endPaintStroke = async () => {
  if (!state.paintStroke) return;
  clearAirbrushTimer();
  const stroke = state.paintStroke;
  state.paintStroke = null;
  const cells = [...stroke.updates.entries()].map(([key, value]) => {
    const [q, r] = key.split(",").map(Number);
    return { q, r, value };
  });
  const spatialStatus = document.querySelector("#spatial-status");
  if (cells.length === 0) {
    if (spatialStatus) spatialStatus.textContent = "Stroke hit no editable cells.";
    invalidateMapCache();
    drawSpatial();
    return { applied_cells: 0, phases: null };
  }
  if (spatialStatus) spatialStatus.textContent = "Saving stroke…";
  try {
    const network = [];
    const commitStarted = performance.now();
    const result = await commitStrokeCells(cells, {
      onTiming: (timing) => network.push(timing),
    });
    const durableAckMs = performance.now() - commitStarted;
    const applyStarted = performance.now();
    if (result?.state) {
      fullApplySpatial(result);
    } else {
      applyStrokeAck(result);
      commitDirtyMapCache();
      drawSpatial();
    }
    const clientApplyMs = performance.now() - applyStarted;
    const opName = ({
      raise: "Raise",
      lower: "Lower",
      flatten: "Flatten",
      smooth: "Smooth",
    })[stroke.op] || (stroke.delta > 0 ? "Raise" : "Lower");
    const modeLabel = stroke.mode === "airbrush" ? `Airbrush ${stroke.rate}/s` : "Stamp";
    if (spatialStatus) spatialStatus.textContent = `${opName} ${modeLabel} r=${stroke.radius} · ${cells.length} cells`;
    return {
      revision: result.revision ?? result.state?.revision,
      applied_cells: result.applied_cells ?? cells.length,
      response_kind: result?.state ? "full_view" : "delta_ack",
      server_timings: result.server_timings ?? null,
      phases: {
        durable_ack_ms: durableAckMs,
        client_apply_ms: clientApplyMs,
        network,
      },
    };
  } catch (error) {
    if (spatialStatus) {
      spatialStatus.textContent = error.status === 409
        ? "Map changed — reloading…"
        : (error.message || "Stroke save failed");
    }
    await loadSpatial().catch(() => {});
    return { error: error.message || "Stroke save failed" };
  }
};
