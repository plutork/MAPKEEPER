#!/usr/bin/env python3
"""CI structural gate for N-026 relief render scale (not flaky ms budgets)."""

from __future__ import annotations

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "docs" / "perf" / "relief-render-scale-report.json"
WEB = ROOT / "crates" / "web"
WASM = WEB / "src" / "lib.rs"
BENCH = ROOT / "scripts" / "bench-render-scale.mjs"
BENCH_LIB = ROOT / "scripts" / "bench-render-scale-lib.mjs"
SCHEMA = "mapkeeper.relief-render-scale.v2"
FIELD_FLUSH_BATCH_MAX = 512


def _shell_sources() -> str:
    parts: list[str] = []
    for name in (
        "index.html",
        "main.js",
        "renderer.js",
        "workspace-state.js",
        "bench-hooks.js",
    ):
        path = WEB / name
        if path.is_file():
            parts.append(path.read_text(encoding="utf-8"))
    return "\n".join(parts)


def harness_revision() -> str:
    h = hashlib.sha256()
    for path in (BENCH, BENCH_LIB):
        h.update(path.read_bytes())
        h.update(b"\0")
    return h.hexdigest()[:16]


def transport_for(changed: int) -> tuple[str, int]:
    if changed <= 0:
        return "none", 0
    if changed <= FIELD_FLUSH_BATCH_MAX:
        return "oneshot", 1
    return "begin_chunk_commit", (changed + FIELD_FLUSH_BATCH_MAX - 1) // FIELD_FLUSH_BATCH_MAX


def assert_changed_cells_truthful(
    label: str, changed: object, catalog: object, transport: object, chunks: object
) -> list[str]:
    errors: list[str] = []
    if not isinstance(changed, int) or changed < 1:
        errors.append(f"{label}: changed_cells must be positive int")
        return errors
    if isinstance(catalog, int) and catalog > 100 and changed == catalog:
        errors.append(
            f"{label}: changed_cells equals catalog_cells ({catalog}) — misleading map-size label"
        )
    expected_t, expected_c = transport_for(changed)
    if transport is not None and transport != expected_t:
        errors.append(f"{label}: transport={transport} expected {expected_t}")
    if chunks is not None and chunks != expected_c:
        errors.append(f"{label}: chunks={chunks} expected {expected_c}")
    return errors


def validate_report(data: dict) -> list[str]:
    errors: list[str] = []
    if data.get("schema") != SCHEMA:
        errors.append(f"report schema must be {SCHEMA}")
    for key in (
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
    ):
        if data.get(key) is None:
            errors.append(f"missing {key}")
    if data.get("evidence_class") != "reproducible_headless":
        errors.append("evidence_class must be reproducible_headless")
    if data.get("supported_sot") != "owner_windows_tauri_release":
        errors.append("supported_sot must be owner_windows_tauri_release")
    hv = data.get("headless_verdict") or {}
    if hv.get("release_gate") == "passed":
        errors.append("headless_verdict must not claim release_gate passed")
    label = hv.get("label") or ""
    if "release gate pending" not in label.lower() and "release_gate_pending" not in label:
        errors.append("headless_verdict.label must state release gate pending")
    rg = data.get("release_gate") or {}
    if rg.get("status") == "passed" and not rg.get("owner_run_at"):
        errors.append("release_gate.status=passed requires owner_run_at")
    mem = data.get("memory") or {}
    if not mem.get("signal"):
        errors.append("memory.signal required")
    if mem.get("reliability") != "proxy_not_process_rss" and "proxy" not in str(
        mem.get("reliability", "")
    ):
        errors.append("memory.reliability must acknowledge proxy (not process RSS)")
    ops = data.get("operations") or {}
    gating = ops.get("gating") or []
    for need in (
        "open_fit",
        "pan",
        "zoom",
        "stamp_drag",
        "airbrush_5",
        "commit_medium",
    ):
        if need not in gating:
            errors.append(f"operations.gating missing {need}")
    ng = ops.get("non_gating_measured") or {}
    for need in ("view_empty", "relief"):
        if need not in ng:
            errors.append(f"operations.non_gating_measured.{need} required")

    sizes = {row.get("size") for row in data.get("matrix") or []}
    for need in ("approx_2k", "approx_10k", "approx_25k", "approx_50k"):
        if need not in sizes:
            errors.append(f"report matrix missing {need}")

    tracked_rev = data.get("harness_revision")
    if tracked_rev and BENCH.is_file() and BENCH_LIB.is_file():
        current = harness_revision()
        if tracked_rev != current:
            errors.append(
                f"harness_revision mismatch: report={tracked_rev} scripts={current} "
                "(re-run node scripts/bench-render-scale.mjs)"
            )

    for size_row in data.get("sizes") or []:
        catalog = size_row.get("catalog_cells")
        commits = size_row.get("commit_strokes") or {}
        if not commits:
            errors.append(f"{size_row.get('size')}: missing commit_strokes")
            continue
        for label, stroke in commits.items():
            errors.extend(
                assert_changed_cells_truthful(
                    f"{size_row.get('size')}.{label}",
                    stroke.get("changed_cells"),
                    catalog,
                    stroke.get("transport"),
                    stroke.get("chunks"),
                )
            )
        legacy = size_row.get("commit_latency_ms")
        if isinstance(legacy, dict) and "cells" in legacy and "changed_cells" not in legacy:
            if legacy.get("cells") == catalog:
                errors.append(
                    f"{size_row.get('size')}: legacy commit_latency_ms.cells == catalog_cells"
                )
    return errors


def main() -> int:
    errors: list[str] = []
    if not REPORT.is_file():
        errors.append(f"missing tracked report: {REPORT.relative_to(ROOT)}")
    else:
        try:
            data = json.loads(REPORT.read_text(encoding="utf-8"))
        except json.JSONDecodeError as e:
            errors.append(f"report JSON invalid: {e}")
            data = None
        if data is not None:
            errors.extend(validate_report(data))
            if "crs_signals" not in (data or {}):
                errors.append("report missing crs_signals")

    shell = _shell_sources()
    wasm = WASM.read_text(encoding="utf-8") if WASM.is_file() else ""
    for token in (
        "viewportCull",
        "offscreenCache",
        "visibleCells",
        "requestAnimationFrame",
        "centerCache",
        "dirtyRect",
        "__MK_BENCH__",
    ):
        if token == "viewportCull":
            if "visibleCells" not in shell and "viewportCull" not in shell:
                errors.append("shell missing viewport cull signal (visibleCells)")
            continue
        if token not in shell:
            errors.append(f"shell missing CRS signal `{token}`")
    if "probe_grid_centers" not in wasm:
        errors.append("wasm missing probe_grid_centers batch helper")
    if re.search(r"for\s*\(\s*let\s+row[\s\S]{0,400}?probe_axial_to_world", shell):
        errors.append("per-cell probe_axial_to_world still inside row loop (CRS regression)")
    if "FIELD_FLUSH_BATCH_MAX = 512" not in (WEB / "workspace-state.js").read_text(
        encoding="utf-8"
    ):
        errors.append("workspace-state FIELD_FLUSH_BATCH_MAX drift vs bench (expected 512)")
    main_js = (WEB / "main.js").read_text(encoding="utf-8") if (WEB / "main.js").is_file() else ""
    if "window.__MK_BENCH__" in main_js:
        errors.append("main.js must not assign window.__MK_BENCH__ on ordinary startup")
    if 'get("bench") === "1"' not in main_js and "get('bench') === '1'" not in main_js:
        errors.append("main.js must load bench-hooks only when ?bench=1")
    if not (WEB / "bench-hooks.js").is_file():
        errors.append("missing crates/web/bench-hooks.js")

    if errors:
        print("check_render_scale_bench: FAIL")
        for e in errors:
            print(f"  - {e}")
        return 1
    print(
        "check_render_scale_bench: OK (structural; headless budgets report-only; "
        "Tauri release gate separate)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
