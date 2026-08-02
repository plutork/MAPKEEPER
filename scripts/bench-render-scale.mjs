/**
 * N-026 relief render scale harness (Playwright + product server).
 * Frame costs separate from stroke commit; commit uses truthful changed_cells.
 *
 * Usage (MAPKEEPER root, server built, web dist present):
 *   node scripts/bench-render-scale.mjs
 * Env:
 *   MAPKEEPER_BENCH_BASE_URL  — reuse server (no spawn)
 *   MAPKEEPER_BENCH_OUT       — report path
 *   MAPKEEPER_BENCH_CI=1      — do not exit 2 on headless budget miss
 */
import { spawn, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import {
  SCHEMA,
  FIELD_FLUSH_BATCH_MAX,
  STROKE_SIZES,
  makeStrokeCells,
  transportForCellCount,
  headlessVerdictLabel,
  OPERATIONS,
} from "./bench-render-scale-lib.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const OUT =
  process.env.MAPKEEPER_BENCH_OUT ||
  path.join(ROOT, "docs", "perf", "relief-render-scale-report.json");
const CI = process.env.MAPKEEPER_BENCH_CI === "1";

const SIZES = [
  { id: "approx_2k", preset_id: "wide_2000", cells: 55 * 36 },
  { id: "approx_10k", preset_id: "wide_12000", cells: 136 * 88 },
  { id: "approx_25k", preset_id: "wide_26000", cells: 200 * 130 },
  { id: "approx_50k", preset_id: "wide_50000", cells: 277 * 180 },
];

const BUDGETS = {
  approx_2k: { open_fit_p95: 100, frame_p95: 16, stamp_p95: 16, airbrush5_p95: 16, commit_p95: 100 },
  approx_10k: { open_fit_p95: 200, frame_p95: 20, stamp_p95: 20, airbrush5_p95: 20, commit_p95: 150 },
  approx_25k: { open_fit_p95: 400, frame_p95: 33, stamp_p95: 33, airbrush5_p95: 33, commit_p95: 250 },
  approx_50k: { open_fit_p95: 800, frame_p95: 33, stamp_p95: 33, airbrush5_p95: 33, commit_p95: 400 },
};

function freePort() {
  return new Promise((resolve, reject) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
    s.on("error", reject);
  });
}

async function waitHealth(base, ms = 45000) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    try {
      const r = await fetch(`${base}/api/health`);
      if (r.ok) return r.json();
    } catch {
      /* retry */
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error("health timeout");
}

function serverBin() {
  const target = process.env.CARGO_TARGET_DIR || path.join(ROOT, "target");
  const name = process.platform === "win32" ? "mapkeeper-server.exe" : "mapkeeper-server";
  return path.join(target, "debug", name);
}

function percentile(arr, p) {
  if (!arr.length) return null;
  const s = [...arr].sort((a, b) => a - b);
  const i = Math.min(s.length - 1, Math.max(0, Math.ceil((p / 100) * s.length) - 1));
  return s[i];
}

function gitSha() {
  try {
    return execSync("git rev-parse HEAD", { cwd: ROOT, encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
}

async function harnessRevision() {
  const files = [
    path.join(__dirname, "bench-render-scale.mjs"),
    path.join(__dirname, "bench-render-scale-lib.mjs"),
  ];
  const h = createHash("sha256");
  for (const f of files) {
    h.update(await readFile(f));
    h.update("\0");
  }
  return h.digest("hex").slice(0, 16);
}

async function measureOneStroke(base, label, changedCells, seed) {
  const spatial = await (await fetch(`${base}/api/spatial`)).json();
  const cols = spatial.state?.grid?.width ?? 55;
  const rows = spatial.state?.grid?.height ?? 36;
  const cells = makeStrokeCells(changedCells, seed, { cols, rows });
  const { transport, chunks } = transportForCellCount(cells.length);
  const baseRevision = spatial.state.revision;
  const strokeId = `bench-${label}-${Date.now()}-${seed}`;

  const tSer0 = performance.now();
  const oneshotBody = JSON.stringify({
    stroke_id: strokeId,
    base_revision: baseRevision,
    mode: "stamp",
    cells,
  });
  const serialize_ms = performance.now() - tSer0;

  let upload_ms = null;
  let server_commit_ms = null;
  let total_ms;
  const t0 = performance.now();

  if (transport === "oneshot") {
    const tUp0 = performance.now();
    const r = await fetch(`${base}/api/spatial/stroke`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: oneshotBody,
    });
    upload_ms = performance.now() - tUp0;
    server_commit_ms = upload_ms; // HTTP round-trip = upload+server (not separable further)
    if (!r.ok) throw new Error(`stroke ${label}: ${await r.text()}`);
    await r.json();
  } else {
    const beginBody = JSON.stringify({
      stroke_id: strokeId,
      base_revision: baseRevision,
      mode: "stamp",
    });
    const tBegin0 = performance.now();
    const b = await fetch(`${base}/api/spatial/stroke/begin`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: beginBody,
    });
    if (!b.ok) throw new Error(`begin ${label}: ${await b.text()}`);
    let chunkUpload = 0;
    for (let c = 0; c < chunks; c++) {
      const slice = cells.slice(c * FIELD_FLUSH_BATCH_MAX, (c + 1) * FIELD_FLUSH_BATCH_MAX);
      const tC0 = performance.now();
      const cr = await fetch(`${base}/api/spatial/stroke/chunk`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          stroke_id: strokeId,
          chunk_id: String(c),
          cells: slice,
        }),
      });
      chunkUpload += performance.now() - tC0;
      if (!cr.ok) throw new Error(`chunk ${label}: ${await cr.text()}`);
    }
    const tCommit0 = performance.now();
    const commitR = await fetch(`${base}/api/spatial/stroke/commit`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ stroke_id: strokeId }),
    });
    server_commit_ms = performance.now() - tCommit0;
    upload_ms = performance.now() - tBegin0 - server_commit_ms;
    if (!commitR.ok) throw new Error(`commit ${label}: ${await commitR.text()}`);
    await commitR.json();
    void chunkUpload;
  }
  total_ms = performance.now() - t0;

  // Confirm revision advanced (applied).
  const after = await (await fetch(`${base}/api/spatial`)).json();
  if ((after.state.revision ?? 0) <= baseRevision) {
    throw new Error(`stroke ${label}: revision did not advance`);
  }

  return {
    label,
    changed_cells: cells.length,
    transport,
    chunks,
    serialize_ms,
    upload_ms,
    server_commit_ms,
    total_ms,
  };
}

async function measureCommitStrokes(base) {
  const out = {};
  for (const [label, n] of Object.entries(STROKE_SIZES)) {
    const samples = [];
    const reps = label === "large" ? 3 : 5;
    for (let i = 0; i < reps; i++) {
      samples.push(await measureOneStroke(base, label, n, i * 10007 + n));
    }
    const totals = samples.map((s) => s.total_ms);
    const { transport, chunks } = transportForCellCount(n);
    out[label] = {
      changed_cells: n,
      transport,
      chunks,
      p50: percentile(totals, 50),
      p95: percentile(totals, 95),
      n: samples.length,
      phases_p95: {
        serialize_ms: percentile(
          samples.map((s) => s.serialize_ms),
          95,
        ),
        upload_ms: percentile(
          samples.map((s) => s.upload_ms).filter((x) => x != null),
          95,
        ),
        server_commit_ms: percentile(
          samples.map((s) => s.server_commit_ms).filter((x) => x != null),
          95,
        ),
      },
      note:
        transport === "oneshot"
          ? "upload_ms ≈ full HTTP round-trip (upload+server not split)"
          : "chunked path: upload_ms includes begin+chunks; server_commit_ms is final commit HTTP",
    };
  }
  return out;
}

function evalBudgets(sizeId, render, commitStrokes) {
  const b = BUDGETS[sizeId];
  const commitMedium = commitStrokes.medium;
  const checks = {
    open_fit: render.open_fit?.p95 != null && render.open_fit.p95 <= b.open_fit_p95,
    pan: render.pan?.p95 != null && render.pan.p95 <= b.frame_p95,
    zoom: render.zoom?.p95 != null && render.zoom.p95 <= b.frame_p95,
    stamp_drag: render.stamp_drag?.p95 != null && render.stamp_drag.p95 <= b.stamp_p95,
    airbrush_5: render.airbrush?.["5"]?.p95 != null && render.airbrush["5"].p95 <= b.airbrush5_p95,
    commit_medium:
      commitMedium?.p95 != null &&
      commitMedium.changed_cells === STROKE_SIZES.medium &&
      commitMedium.p95 <= b.commit_p95,
  };
  const gating_pass = Object.values(checks).every(Boolean);
  return {
    budgets: b,
    checks,
    /** @deprecated use headless_provisionally_supported — not Tauri SoT */
    supported: gating_pass,
    headless_provisionally_supported: gating_pass,
    non_gating: {
      view_empty_p95: render.view_empty?.p95 ?? null,
      relief_p95: render.relief?.p95 ?? null,
      commit_small_p95: commitStrokes.small?.p95 ?? null,
      commit_large_p95: commitStrokes.large?.p95 ?? null,
      commit_large_changed_cells: commitStrokes.large?.changed_cells ?? null,
      commit_large_transport: commitStrokes.large?.transport ?? null,
    },
  };
}

async function sampleMemory(page) {
  // Chromium JS heap via CDP — proxy, not process RSS / Tauri Working Set.
  let js_heap_used_bytes = null;
  let js_heap_total_bytes = null;
  try {
    const client = await page.context().newCDPSession(page);
    await client.send("Performance.enable");
    const { metrics } = await client.send("Performance.getMetrics");
    const used = metrics.find((m) => m.name === "JSHeapUsedSize");
    const total = metrics.find((m) => m.name === "JSHeapTotalSize");
    js_heap_used_bytes = used?.value ?? null;
    js_heap_total_bytes = total?.value ?? null;
    await client.detach().catch(() => {});
  } catch {
    /* CDP unavailable */
  }
  const node = process.memoryUsage();
  return {
    signal: "chromium_js_heap_used_bytes",
    reliability: "proxy_not_process_rss",
    note:
      "Headless Chromium JS heap via CDP Performance.getMetrics. Not OS process RSS. Owner Tauri release must record Working Set / private bytes separately.",
    js_heap_used_bytes,
    js_heap_total_bytes,
    node_rss_bytes: node.rss,
    node_heap_used_bytes: node.heapUsed,
  };
}

async function main() {
  const tempRoot = path.join(
    process.env.TEMP || process.env.TMPDIR || "/tmp",
    `mk-bench-${Date.now()}`,
  );
  await mkdir(tempRoot, { recursive: true });
  process.env.APPDATA = path.join(tempRoot, "appdata");
  process.env.HOME = path.join(tempRoot, "home");

  let child = null;
  let base = process.env.MAPKEEPER_BENCH_BASE_URL;
  if (!base) {
    const webDist = path.join(ROOT, "crates", "web", "dist");
    if (!existsSync(webDist)) {
      throw new Error("crates/web/dist missing — run crates/web/build.ps1 first");
    }
    const port = await freePort();
    base = `http://127.0.0.1:${port}`;
    const bin = serverBin();
    if (!existsSync(bin)) {
      throw new Error(`server binary missing: ${bin} (cargo build -p mapkeeper-server)`);
    }
    child = spawn(bin, ["--port", String(port), "--web-dist", webDist], {
      cwd: ROOT,
      stdio: "ignore",
      env: process.env,
    });
    await waitHealth(base);
  }

  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const results = [];
  const memoryBySize = {};

  try {
    for (const size of SIZES) {
      const worldPath = path.join(tempRoot, "worlds", size.id);
      const create = await fetch(`${base}/api/projects`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          id: `bench-${size.id}`,
          path: worldPath,
          preset_id: size.preset_id,
        }),
      });
      if (!create.ok) throw new Error(`create ${size.id}: ${await create.text()}`);

      const tOpen0 = performance.now();
      const benchUrl = `${base}/?bench=1`;
      await page.goto(benchUrl, { waitUntil: "networkidle" });
      await page.waitForFunction(() => window.__MK_BENCH__);
      await page.evaluate(async (p) => {
        await fetch("/api/projects/open", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path: p }),
        });
        location.href = "/?bench=1";
      }, worldPath);
      await page.waitForFunction(() => window.__MK_BENCH__);
      await page.waitForFunction(() => window.__MK_BENCH__.cellCount() > 0, null, {
        timeout: 60000,
      });
      const openNavMs = performance.now() - tOpen0;

      const render = await page.evaluate(async () => window.__MK_BENCH__.runSuite());
      const commit_strokes = await measureCommitStrokes(base);
      const verdict = evalBudgets(size.id, render, commit_strokes);
      memoryBySize[size.id] = await sampleMemory(page);

      const tReload0 = performance.now();
      await page.goto(benchUrl, { waitUntil: "networkidle" });
      await page.waitForFunction(() => window.__MK_BENCH__ && window.__MK_BENCH__.cellCount() > 0);
      await page.evaluate(() => window.__MK_BENCH__.flushDraw());
      const reload_ms = performance.now() - tReload0;

      results.push({
        size: size.id,
        preset_id: size.preset_id,
        catalog_cells: size.cells,
        measured_cells: render.cells,
        open_nav_ms: openNavMs,
        reload_after_stroke_ms: reload_ms,
        render,
        commit_strokes,
        verdict,
      });

      await fetch(`${base}/api/projects/close`, { method: "POST" });
    }
  } finally {
    await browser.close();
    if (child) child.kill();
  }

  let previous = null;
  try {
    previous = JSON.parse(await readFile(OUT, "utf8"));
  } catch {
    /* first run */
  }

  const allPass = results.every((r) => r.verdict.headless_provisionally_supported);
  const harness_revision = await harnessRevision();
  const report = {
    schema: SCHEMA,
    generated_at: new Date().toISOString(),
    git_sha: gitSha(),
    build_mode: "debug-server + web dist",
    platform: `${process.platform}-${process.arch}`,
    harness_revision,
    phase: "after_crs",
    surface: "playwright-chromium-headless",
    evidence_class: "reproducible_headless",
    supported_sot: "owner_windows_tauri_release",
    headless_verdict: {
      label: headlessVerdictLabel(allPass),
      release_gate: "pending",
      all_sizes_gating_pass: allPass,
    },
    release_gate: {
      status: "pending",
      instruction_path: "docs/perf/OWNER-TAURI-RELEASE-GATE.md",
      owner_run_at: null,
      note: "Not claimed in this harness run. Owner Windows Tauri release remains product SoT.",
    },
    memory: {
      signal: "chromium_js_heap_used_bytes",
      reliability: "proxy_not_process_rss",
      note:
        "CDP JS heap is a reproducible proxy on headless Chromium. Mandatory owner Tauri release observation: process Working Set after open at each size.",
      by_size: memoryBySize,
      owner_tauri_observation_required: true,
    },
    operations: OPERATIONS,
    field_flush_batch_max: FIELD_FLUSH_BATCH_MAX,
    stroke_sizes: STROKE_SIZES,
    note_facts:
      "Frame times from flushDrawSpatial (paint). Commit uses truthful changed_cells (small/medium/large); large uses begin/chunk/commit when >512. commit_latency_ms.cells=map-size was a bug — removed. Headless Chromium is reproducible evidence, not final Supported SoT.",
    note_assumptions:
      "Supported SoT = owner Windows Tauri release. Until that run: «provisionally supported on headless benchmark surface; release gate pending». View Empty / Relief full rebuilds are non-gating measured ops.",
    crs_signals: {
      viewportCull: true,
      offscreenCache: true,
      rafCoalesce: true,
      centerCache: true,
      dirtyRect: true,
    },
    sizes: results,
    previous_phase: previous?.phase ?? null,
    matrix: results.map((r) => ({
      size: r.size,
      headless_provisionally_supported: r.verdict.headless_provisionally_supported,
      supported: r.verdict.headless_provisionally_supported,
      checks: r.verdict.checks,
      open_fit_p95: r.render.open_fit?.p95,
      pan_p95: r.render.pan?.p95,
      stamp_p95: r.render.stamp_drag?.p95,
      airbrush5_p95: r.render.airbrush?.["5"]?.p95,
      commit_medium_p95: r.commit_strokes.medium?.p95,
      commit_medium_changed_cells: r.commit_strokes.medium?.changed_cells,
      view_empty_p95: r.render.view_empty?.p95,
      relief_p95: r.render.relief?.p95,
      commit_large_p95: r.commit_strokes.large?.p95,
      commit_large_changed_cells: r.commit_strokes.large?.changed_cells,
      commit_large_transport: r.commit_strokes.large?.transport,
    })),
    before_crs_matrix: previous?.before_crs_matrix ?? null,
    after_crs_matrix: null,
  };
  report.after_crs_matrix = report.matrix;

  await mkdir(path.dirname(OUT), { recursive: true });
  await writeFile(OUT, JSON.stringify(report, null, 2));
  console.log(`Wrote ${OUT}`);
  console.log(`headless_verdict: ${report.headless_verdict.label}`);
  console.log(`release_gate: ${report.release_gate.status}`);
  for (const row of report.matrix) {
    console.log(
      `${row.size}: provisional=${row.headless_provisionally_supported} open=${row.open_fit_p95?.toFixed?.(1)} commit_med=${row.commit_medium_p95?.toFixed?.(1)}(cells=${row.commit_medium_changed_cells}) large=${row.commit_large_p95?.toFixed?.(1)}(cells=${row.commit_large_changed_cells},${row.commit_large_transport})`,
    );
  }

  if (!CI && !allPass) {
    console.log(
      "FACT: headless gating budgets missed — not a Tauri SoT fail; re-check on owner release gate.",
    );
    process.exitCode = 2;
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
