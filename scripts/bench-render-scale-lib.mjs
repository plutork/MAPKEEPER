/**
 * Pure helpers for N-026 relief render-scale harness (no Playwright).
 * FIELD_FLUSH_BATCH_MAX must match crates/web/workspace-state.js.
 */

export const SCHEMA = "mapkeeper.relief-render-scale.v2";
export const FIELD_FLUSH_BATCH_MAX = 512;

/** Representative stroke sizes: cells actually sent/applied. */
export const STROKE_SIZES = {
  small: 1,
  medium: 64,
  large: 1200, // > batch max → begin/chunk/commit
};

/** odd-r offset → axial (matches crates/core/src/spatial/grid.rs). */
export function offsetToAxial(col, row) {
  const q = col - (row - (row & 1)) / 2;
  return { q, r: row };
}

/**
 * Build axial cells inside an active grid via offset lattice (defaults ~2k).
 * When n > cols*rows, later writes overwrite earlier keys — payload length
 * stays truthful for transport/commit timing.
 */
export function makeStrokeCells(
  changedCells,
  seed = 0,
  grid = { cols: 55, rows: 36 },
) {
  const n = Math.max(0, Math.floor(changedCells));
  const cols = Math.max(1, Math.floor(grid.cols || 55));
  const rows = Math.max(1, Math.floor(grid.rows || 36));
  const cells = [];
  for (let i = 0; i < n; i++) {
    const idx = seed + i;
    const col = idx % cols;
    const row = Math.floor(idx / cols) % rows;
    const { q, r } = offsetToAxial(col, row);
    cells.push({ q, r, value: 1 + (i % 3) });
  }
  return cells;
}

export function transportForCellCount(changedCells, batchMax = FIELD_FLUSH_BATCH_MAX) {
  if (changedCells <= 0) return { transport: "none", chunks: 0 };
  if (changedCells <= batchMax) {
    return { transport: "oneshot", chunks: 1 };
  }
  return {
    transport: "begin_chunk_commit",
    chunks: Math.ceil(changedCells / batchMax),
  };
}

/** Truth: changed_cells is payload size, never catalog map size by coincidence of misuse. */
export function assertChangedCellsTruthful(entry) {
  const errors = [];
  if (!entry || typeof entry !== "object") {
    return ["commit entry missing"];
  }
  const { label, changed_cells: changed, catalog_cells: catalog, transport, chunks } = entry;
  if (!Number.isInteger(changed) || changed < 1) {
    errors.push(`${label}: changed_cells must be positive integer`);
  }
  if (Number.isInteger(catalog) && catalog > 100 && changed === catalog) {
    errors.push(
      `${label}: changed_cells equals catalog_cells (${catalog}) — likely mislabeled map size`,
    );
  }
  const expected = transportForCellCount(changed);
  if (transport && transport !== expected.transport) {
    errors.push(
      `${label}: transport=${transport} but changed_cells=${changed} expects ${expected.transport}`,
    );
  }
  if (chunks != null && chunks !== expected.chunks) {
    errors.push(`${label}: chunks=${chunks} expected ${expected.chunks}`);
  }
  return errors;
}

export function headlessVerdictLabel(allGatingPass) {
  if (allGatingPass) {
    return "provisionally_supported_on_headless_benchmark_surface; release_gate_pending";
  }
  return "headless_budgets_failed; release_gate_pending";
}

export const OPERATIONS = {
  gating: ["open_fit", "pan", "zoom", "stamp_drag", "airbrush_5", "commit_medium"],
  non_gating_measured: {
    view_empty: {
      role: "View Empty full-layer rebuild",
      budget: null,
      note: "Measured; not part of Supported gate (full invalidate path).",
    },
    relief: {
      role: "Relief display full-layer rebuild",
      budget: null,
      note: "Measured; not part of Supported gate (full invalidate path).",
    },
    hover: {
      role: "hover overlay",
      budget: null,
      note: "Measured; degraded OK per N-026.",
    },
    radius_preview: {
      role: "brush radius preview",
      budget: null,
      note: "Measured; non-gating.",
    },
    commit_small: {
      role: "1-cell oneshot commit",
      budget: null,
      note: "Measured for scale; Supported gates on commit_medium.",
    },
    commit_large: {
      role: "many-cell begin/chunk/commit",
      budget: null,
      note: "Measured separately; must use real multi-cell payload.",
    },
    airbrush_20: {
      role: "Airbrush 20 soft-fail OK",
      budget: null,
      note: "May soft-fail at 25k/50k if Airbrush 5 holds (N-026).",
    },
  },
};

export function validateReportSchema(data) {
  const errors = [];
  if (!data || typeof data !== "object") return ["report not an object"];
  if (data.schema !== SCHEMA) {
    errors.push(`schema must be ${SCHEMA}`);
  }
  for (const key of [
    "generated_at",
    "git_sha",
    "build_mode",
    "platform",
    "harness_revision",
    "surface",
    "evidence_class",
    "supported_sot",
    "headless_verdict",
    "release_gate",
    "memory",
    "operations",
    "note_facts",
    "note_assumptions",
    "crs_signals",
    "matrix",
    "sizes",
  ]) {
    if (data[key] == null) errors.push(`missing ${key}`);
  }
  if (data.evidence_class !== "reproducible_headless") {
    errors.push("evidence_class must be reproducible_headless");
  }
  if (data.supported_sot !== "owner_windows_tauri_release") {
    errors.push("supported_sot must be owner_windows_tauri_release");
  }
  if (data.release_gate?.status === "passed" && !data.release_gate?.owner_run_at) {
    errors.push("release_gate.status=passed requires owner_run_at");
  }
  if (data.headless_verdict?.release_gate === "passed") {
    errors.push("headless_verdict must not claim release_gate passed");
  }
  const sizes = new Set((data.matrix || []).map((r) => r.size));
  for (const need of ["approx_2k", "approx_10k", "approx_25k", "approx_50k"]) {
    if (!sizes.has(need)) errors.push(`matrix missing ${need}`);
  }
  for (const sizeRow of data.sizes || []) {
    const catalog = sizeRow.catalog_cells;
    const commits = sizeRow.commit_strokes || {};
    for (const [label, stroke] of Object.entries(commits)) {
      errors.push(
        ...assertChangedCellsTruthful({
          label: `${sizeRow.size}.${label}`,
          changed_cells: stroke.changed_cells,
          catalog_cells: catalog,
          transport: stroke.transport,
          chunks: stroke.chunks,
        }),
      );
    }
    // Legacy trap: top-level commit_latency_ms.cells == catalog
    const legacy = sizeRow.commit_latency_ms;
    if (legacy && legacy.cells != null && legacy.changed_cells == null) {
      if (legacy.cells === catalog) {
        errors.push(
          `${sizeRow.size}: legacy commit_latency_ms.cells equals catalog_cells (misleading)`,
        );
      }
    }
  }
  if (data.operations?.gating) {
    for (const g of OPERATIONS.gating) {
      if (!data.operations.gating.includes(g)) {
        errors.push(`operations.gating missing ${g}`);
      }
    }
  }
  if (!data.operations?.non_gating_measured?.view_empty) {
    errors.push("operations.non_gating_measured.view_empty required");
  }
  if (!data.operations?.non_gating_measured?.relief) {
    errors.push("operations.non_gating_measured.relief required");
  }
  if (data.memory && !data.memory.signal) {
    errors.push("memory.signal required");
  }
  return errors;
}
