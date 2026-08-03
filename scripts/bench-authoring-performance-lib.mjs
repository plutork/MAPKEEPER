import { offsetToAxial } from "./bench-render-scale-lib.mjs";

export const AUTHORING_SCHEMA = "mapkeeper.continuous-authoring.v1";
export const DENSITY_PCTS = [0, 25, 75, 100];
export const AUTHORING_BUDGET_P95_MS = 100;

const seededRandom = (seed) => {
  let value = seed >>> 0;
  return () => {
    value ^= value << 13;
    value ^= value >>> 17;
    value ^= value << 5;
    return (value >>> 0) / 0x100000000;
  };
};

/** Deterministic exact-density relief cells for a mature benchmark map. */
export function makeMatureReliefCells(width, height, densityPct, seed = 39) {
  const cols = Math.max(1, Math.floor(width));
  const rows = Math.max(1, Math.floor(height));
  const pct = Math.max(0, Math.min(100, Number(densityPct) || 0));
  const total = cols * rows;
  const count = Math.floor(total * pct / 100);
  const order = Array.from({ length: total }, (_, i) => i);
  const random = seededRandom(seed + total * 17 + count);
  for (let i = order.length - 1; i > 0; i--) {
    const j = Math.floor(random() * (i + 1));
    [order[i], order[j]] = [order[j], order[i]];
  }
  const cells = {};
  for (let i = 0; i < count; i++) {
    const index = order[i];
    const col = index % cols;
    const row = Math.floor(index / cols);
    const { q, r } = offsetToAxial(col, row);
    cells[`${q},${r}`] = 1 + ((index * 17 + seed) % 40);
  }
  return cells;
}

export function validateAuthoringReport(report) {
  const errors = [];
  if (report?.schema !== AUTHORING_SCHEMA) {
    errors.push(`schema must be ${AUTHORING_SCHEMA}`);
  }
  for (const key of [
    "generated_at",
    "git_sha",
    "harness_revision",
    "phase",
    "evidence_class",
    "supported_sot",
    "release_gate",
    "contract",
    "matrix",
  ]) {
    if (report?.[key] == null) errors.push(`missing ${key}`);
  }
  if (report?.evidence_class !== "reproducible_headless") {
    errors.push("headless evidence must stay report-only");
  }
  if (report?.supported_sot !== "owner_windows_tauri_release") {
    errors.push("Supported SoT must be owner Windows Tauri release");
  }
  if (report?.release_gate?.status === "passed" && !report.release_gate.owner_run_at) {
    errors.push("passed release gate requires owner_run_at");
  }
  const rows = report?.matrix ?? [];
  const keys = new Set(rows.map((row) => `${row.size}:${row.density_pct}`));
  for (const size of ["approx_2k", "approx_12k", "approx_26k", "approx_50k"]) {
    for (const density of DENSITY_PCTS) {
      if (!keys.has(`${size}:${density}`)) {
        errors.push(`matrix missing ${size}:${density}`);
      }
    }
  }
  for (const row of rows) {
    if (!Object.hasOwn(row.memory ?? {}, "server_process_working_set_bytes")) {
      errors.push(`${row.size}:${row.density_pct} missing process memory signal`);
    }
    if (row.authoring?.budget_p95_ms !== AUTHORING_BUDGET_P95_MS) {
      errors.push(`${row.size}:${row.density_pct} budget drift`);
    }
    for (const name of ["small", "medium", "series_100_small"]) {
      const sample = row.authoring?.[name];
      if (!sample || sample.p95 == null || !sample.phases_p95) {
        errors.push(`${row.size}:${row.density_pct} missing ${name} timings`);
      }
    }
  }
  return errors;
}
