#!/usr/bin/env python3
"""CI structural gate for N-026 relief render scale (not flaky ms budgets)."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REPORT = ROOT / "docs" / "perf" / "relief-render-scale-report.json"
WEB = ROOT / "crates" / "web"
WASM = WEB / "src" / "lib.rs"


def _shell_sources() -> str:
    parts: list[str] = []
    for name in ("index.html", "main.js", "renderer.js", "workspace-state.js"):
        path = WEB / name
        if path.is_file():
            parts.append(path.read_text(encoding="utf-8"))
    return "\n".join(parts)


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
            if data.get("schema") != "mapkeeper.relief-render-scale.v1":
                errors.append("report schema must be mapkeeper.relief-render-scale.v1")
            sizes = {row.get("size") for row in data.get("matrix") or []}
            for need in ("approx_2k", "approx_10k", "approx_25k", "approx_50k"):
                if need not in sizes:
                    errors.append(f"report matrix missing {need}")
            if "crs_signals" not in data:
                errors.append("report missing crs_signals")
            if "note_facts" not in data or "note_assumptions" not in data:
                errors.append("report must separate note_facts and note_assumptions")

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

    if errors:
        print("check_render_scale_bench: FAIL")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("check_render_scale_bench: OK (structural; budgets are report-only in CI)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
