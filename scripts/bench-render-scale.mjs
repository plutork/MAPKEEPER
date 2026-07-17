/**
 * N-026 relief render scale harness (Playwright + product server).
 * Measures canvas frame costs separately from stroke commit latency.
 *
 * Usage (from MAPKEEPER root, server already built, web dist present):
 *   node scripts/bench-render-scale.mjs
 * Env:
 *   MAPKEEPER_BENCH_BASE_URL  — if set, reuse server (no spawn)
 *   MAPKEEPER_BENCH_OUT       — report path (default docs/perf/relief-render-scale-report.json)
 *   MAPKEEPER_BENCH_CI=1      — structural/complete only; do not fail on budget ms
 */
import { spawn } from "node:child_process";
import { createServer } from "node:net";
import { mkdir, writeFile, readFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

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

async function measureCommit(base, cells) {
  const samples = [];
  for (let i = 0; i < 5; i++) {
    const spatial = await (await fetch(`${base}/api/spatial`)).json();
    const body = {
      stroke_id: `bench-${Date.now()}-${i}`,
      base_revision: spatial.state.revision,
      mode: "stamp",
      cells: [{ q: i % 8, r: i % 6, value: 1 + (i % 3) }],
    };
    const t0 = performance.now();
    const r = await fetch(`${base}/api/spatial/stroke`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!r.ok) throw new Error(`stroke failed: ${await r.text()}`);
    samples.push(performance.now() - t0);
  }
  return { p50: percentile(samples, 50), p95: percentile(samples, 95), n: samples.length, cells };
}

function evalBudgets(sizeId, render, commit) {
  const b = BUDGETS[sizeId];
  const checks = {
    open_fit: render.open_fit?.p95 != null && render.open_fit.p95 <= b.open_fit_p95,
    pan: render.pan?.p95 != null && render.pan.p95 <= b.frame_p95,
    zoom: render.zoom?.p95 != null && render.zoom.p95 <= b.frame_p95,
    stamp_drag: render.stamp_drag?.p95 != null && render.stamp_drag.p95 <= b.stamp_p95,
    airbrush_5: render.airbrush?.["5"]?.p95 != null && render.airbrush["5"].p95 <= b.airbrush5_p95,
    commit: commit.p95 != null && commit.p95 <= b.commit_p95,
  };
  const supported = Object.values(checks).every(Boolean);
  return { budgets: b, checks, supported };
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
  let phase = "after_crs";

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
      await page.goto(base, { waitUntil: "networkidle" });
      await page.waitForFunction(() => window.__MK_BENCH__);
      // Workspace already active after create; reload spatial via navigate+open if needed.
      await page.evaluate(async (p) => {
        await fetch("/api/projects/open", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ path: p }),
        });
        location.reload();
      }, worldPath);
      await page.waitForFunction(() => window.__MK_BENCH__);
      // Wait for spatial load by polling cellCount
      await page.waitForFunction(() => window.__MK_BENCH__.cellCount() > 0, null, {
        timeout: 60000,
      });
      const openNavMs = performance.now() - tOpen0;

      const render = await page.evaluate(async () => window.__MK_BENCH__.runSuite());
      const commit = await measureCommit(base, size.cells);
      const verdict = evalBudgets(size.id, render, commit);

      // reload after stroke (commit already mutated)
      const tReload0 = performance.now();
      await page.reload({ waitUntil: "networkidle" });
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
        commit_latency_ms: commit,
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

  const report = {
    schema: "mapkeeper.relief-render-scale.v1",
    generated_at: new Date().toISOString(),
    phase,
    surface: "playwright-chromium-headless",
    build: "debug-server + web dist",
    note_facts:
      "Frame times from flushDrawSpatial (includes paint). Commit latency is HTTP stroke only. Headless Chromium ≠ Tauri release gate machine (N-026).",
    note_assumptions:
      "Gate Supported SoT remains owner Windows Tauri release; this report is reproducible CI/maintainer evidence.",
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
      supported: r.verdict.supported,
      checks: r.verdict.checks,
      open_fit_p95: r.render.open_fit?.p95,
      pan_p95: r.render.pan?.p95,
      stamp_p95: r.render.stamp_drag?.p95,
      airbrush5_p95: r.render.airbrush?.["5"]?.p95,
      commit_p95: r.commit_latency_ms.p95,
    })),
    before_crs_matrix: previous?.before_crs_matrix ?? previous?.matrix ?? null,
    after_crs_matrix: null, // filled below
  };
  report.after_crs_matrix = report.matrix;

  await mkdir(path.dirname(OUT), { recursive: true });
  await writeFile(OUT, JSON.stringify(report, null, 2));
  console.log(`Wrote ${OUT}`);
  for (const row of report.matrix) {
    console.log(
      `${row.size}: supported=${row.supported} open=${row.open_fit_p95?.toFixed?.(1)} pan=${row.pan_p95?.toFixed?.(1)} stamp=${row.stamp_p95?.toFixed?.(1)} ab5=${row.airbrush5_p95?.toFixed?.(1)} commit=${row.commit_p95?.toFixed?.(1)}`,
    );
  }

  if (!CI) {
    const fail50 = results.find((r) => r.size === "approx_50k" && !r.verdict.supported);
    if (fail50) {
      console.log(
        "FACT: approx_50k not Supported under this harness surface — apply N-026 ceiling rule on gate machine confirmation / or amend Create catalog.",
      );
      process.exitCode = 2;
    }
  }
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
