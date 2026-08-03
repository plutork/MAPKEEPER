/**
 * N-039 mature-map continuous authoring harness.
 * Headless results are reproducible evidence; owner Tauri release is Supported SoT.
 */
import { spawn, execSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createServer } from "node:net";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";
import {
  AUTHORING_SCHEMA,
  AUTHORING_BUDGET_P95_MS,
  DENSITY_PCTS,
  makeMatureReliefCells,
  validateAuthoringReport,
} from "./bench-authoring-performance-lib.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");
const OUT = process.env.MAPKEEPER_AUTHORING_OUT
  || path.join(ROOT, "docs", "perf", "large-map-authoring-report.json");
const PHASE = process.env.MAPKEEPER_AUTHORING_PHASE || "after_delta_ack";
const FILTER = process.env.MAPKEEPER_AUTHORING_FILTER || "";
const SIZES = [
  { id: "approx_2k", preset_id: "wide_2000", cells: 55 * 36 },
  { id: "approx_12k", preset_id: "wide_12000", cells: 136 * 88 },
  { id: "approx_26k", preset_id: "wide_26000", cells: 200 * 130 },
  { id: "approx_50k", preset_id: "wide_50000", cells: 277 * 180 },
];

const freePort = () => new Promise((resolve, reject) => {
  const server = createServer();
  server.listen(0, "127.0.0.1", () => {
    const { port } = server.address();
    server.close(() => resolve(port));
  });
  server.on("error", reject);
});

const waitHealth = async (base, timeoutMs = 45000) => {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const response = await fetch(`${base}/api/health`);
      if (response.ok) return;
    } catch {
      // Retry startup.
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error("health timeout");
};

const serverBin = () => {
  const target = process.env.CARGO_TARGET_DIR || path.join(ROOT, "target");
  return path.join(target, "debug", process.platform === "win32"
    ? "mapkeeper-server.exe"
    : "mapkeeper-server");
};

const gitSha = () => {
  try {
    return execSync("git rev-parse HEAD", { cwd: ROOT, encoding: "utf8" }).trim();
  } catch {
    return "unknown";
  }
};

const processWorkingSet = async (pid) => {
  if (!pid) return null;
  try {
    if (process.platform === "win32") {
      return Number(execSync(
        `powershell -NoProfile -Command "(Get-Process -Id ${Number(pid)}).WorkingSet64"`,
        { encoding: "utf8" },
      ).trim());
    }
    const status = await readFile(`/proc/${Number(pid)}/status`, "utf8");
    const kib = Number(status.match(/^VmRSS:\s+(\d+)\s+kB$/m)?.[1]);
    return Number.isFinite(kib) ? kib * 1024 : null;
  } catch {
    return null;
  }
};

const harnessRevision = async () => {
  const hash = createHash("sha256");
  for (const relative of [
    "scripts/bench-authoring-performance.mjs",
    "scripts/bench-authoring-performance-lib.mjs",
    "crates/web/bench-hooks.js",
    "crates/web/spatial-transaction.js",
    "crates/web/api.js",
  ]) {
    hash.update(await readFile(path.join(ROOT, relative)));
    hash.update("\0");
  }
  return hash.digest("hex").slice(0, 16);
};

const launchBrowser = async () => {
  const channel = process.env.MAPKEEPER_BROWSER_CHANNEL;
  if (channel) return chromium.launch({ headless: true, channel });
  try {
    return await chromium.launch({ headless: true });
  } catch (error) {
    if (process.platform !== "win32" || !String(error).includes("Executable doesn't exist")) {
      throw error;
    }
    return chromium.launch({ headless: true, channel: "msedge" });
  }
};

const seedMatureRelief = async (base, worldPath, densityPct, seed) => {
  const response = await fetch(`${base}/api/spatial`);
  if (!response.ok) throw new Error(`seed GET spatial: ${await response.text()}`);
  const view = await response.json();
  const { width, height } = view.state.grid;
  view.state.field.cells = makeMatureReliefCells(width, height, densityPct, seed);
  view.state.revision = 1;
  await fetch(`${base}/api/projects/close`, { method: "POST" });
  const statePath = path.join(worldPath, "maps", "main", "spatial", "state.json");
  await writeFile(statePath, JSON.stringify(view.state, null, 2), "utf8");
  return Object.keys(view.state.field.cells).length;
};

const openWorldInBench = async (page, base, worldPath) => {
  const benchUrl = `${base}/?bench=1`;
  await page.goto(benchUrl, { waitUntil: "networkidle" });
  await page.waitForFunction(() => window.__MK_BENCH__);
  await page.evaluate(async (target) => {
    const response = await fetch("/api/projects/open", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ path: target }),
    });
    if (!response.ok) throw new Error(await response.text());
    location.href = "/?bench=1";
  }, worldPath);
  await page.waitForFunction(
    () => window.__MK_BENCH__ && window.__MK_BENCH__.cellCount() > 0,
    null,
    { timeout: 60000 },
  );
};

const main = async () => {
  const tempRoot = path.join(
    process.env.TEMP || process.env.TMPDIR || "/tmp",
    `mk-authoring-${Date.now()}`,
  );
  await mkdir(tempRoot, { recursive: true });
  const env = {
    ...process.env,
    APPDATA: path.join(tempRoot, "appdata"),
    HOME: path.join(tempRoot, "home"),
  };

  let child = null;
  let base = process.env.MAPKEEPER_BENCH_BASE_URL;
  if (!base) {
    const webDist = path.join(ROOT, "crates", "web", "dist");
    if (!existsSync(webDist)) throw new Error("crates/web/dist missing — run crates/web/build.ps1");
    const bin = serverBin();
    if (!existsSync(bin)) throw new Error(`server binary missing: ${bin}`);
    const port = await freePort();
    base = `http://127.0.0.1:${port}`;
    child = spawn(bin, ["--port", String(port), "--web-dist", webDist], {
      cwd: ROOT,
      env,
      stdio: "ignore",
    });
    await waitHealth(base);
  }

  const browser = await launchBrowser();
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const matrix = [];
  try {
    for (const size of SIZES) {
      for (const densityPct of DENSITY_PCTS) {
        const fixtureKey = `${size.id}:${densityPct}`;
        if (FILTER && !fixtureKey.includes(FILTER)) continue;
        const worldPath = path.join(tempRoot, "worlds", `${size.id}-d${densityPct}`);
        const create = await fetch(`${base}/api/projects`, {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            id: `authoring-${size.id}-d${densityPct}`,
            path: worldPath,
            preset_id: size.preset_id,
          }),
        });
        if (!create.ok) throw new Error(`create ${fixtureKey}: ${await create.text()}`);
        const populatedCells = await seedMatureRelief(
          base,
          worldPath,
          densityPct,
          size.cells + densityPct,
        );
        await openWorldInBench(page, base, worldPath);
        const authoring = await page.evaluate(
          async () => window.__MK_BENCH__.runAuthoringSuite(),
        );
        const memory = await page.evaluate(() => ({
          js_heap_used_bytes: performance.memory?.usedJSHeapSize ?? null,
          js_heap_total_bytes: performance.memory?.totalJSHeapSize ?? null,
        }));
        memory.server_process_working_set_bytes = await processWorkingSet(child?.pid);
        memory.note = child
          ? "server process Working Set plus Chromium JS heap proxy"
          : "external server process Working Set unavailable; Chromium JS heap proxy only";
        matrix.push({
          size: size.id,
          preset_id: size.preset_id,
          catalog_cells: size.cells,
          density_pct: densityPct,
          populated_cells: populatedCells,
          authoring,
          memory,
          headless_budget_observed:
            authoring.small.p95 <= AUTHORING_BUDGET_P95_MS
            && authoring.medium.p95 <= AUTHORING_BUDGET_P95_MS
            && authoring.series_100_small.p95 <= AUTHORING_BUDGET_P95_MS,
        });
        console.log(
          `${fixtureKey} small=${authoring.small.p95.toFixed(1)} `
          + `medium=${authoring.medium.p95.toFixed(1)} `
          + `series=${authoring.series_100_small.p95.toFixed(1)} ms`,
        );
        await fetch(`${base}/api/projects/close`, { method: "POST" });
      }
    }
  } finally {
    await browser.close();
    if (child) child.kill();
  }

  let previous = null;
  try {
    previous = JSON.parse(await readFile(OUT, "utf8"));
  } catch {
    // First phase.
  }
  const fullMatrix = !FILTER;
  const baselineMatrix = PHASE === "baseline_full_view"
    ? matrix
    : (previous?.baseline_matrix ?? null);
  const afterMatrix = PHASE === "after_delta_ack"
    ? matrix
    : (previous?.after_delta_ack_matrix ?? null);
  const report = {
    schema: AUTHORING_SCHEMA,
    generated_at: new Date().toISOString(),
    git_sha: gitSha(),
    build_mode: "debug-server + web dist",
    platform: `${process.platform}-${process.arch}`,
    harness_revision: await harnessRevision(),
    phase: PHASE,
    evidence_class: "reproducible_headless",
    supported_sot: "owner_windows_tauri_release",
    release_gate: {
      status: "pending",
      owner_run_at: null,
      instruction_path: "docs/perf/OWNER-TAURI-RELEASE-GATE.md",
    },
    contract: {
      metric: "mouseup_to_durable_ack_first_correct_frame_and_next_stroke_ready",
      p95_budget_ms: AUTHORING_BUDGET_P95_MS,
      sizes: SIZES.map((size) => size.id),
      density_pcts: DENSITY_PCTS,
      next_stroke_waits_for_ack: true,
    },
    note:
      "Headless values are report-only. Supported requires owner Windows Tauri release on the full mature matrix.",
    filter: FILTER || null,
    matrix,
    baseline_matrix: baselineMatrix,
    after_delta_ack_matrix: afterMatrix,
  };
  if (fullMatrix) {
    const errors = validateAuthoringReport(report);
    if (errors.length) throw new Error(`invalid authoring report:\n${errors.join("\n")}`);
  }
  await mkdir(path.dirname(OUT), { recursive: true });
  await writeFile(OUT, JSON.stringify(report, null, 2));
  console.log(`Wrote ${OUT}`);
};

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
