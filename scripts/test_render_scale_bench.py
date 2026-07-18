#!/usr/bin/env python3
"""Unit tests for N-026 bench report schema + truthful changed_cells."""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))

import check_render_scale_bench as check  # noqa: E402


class ChangedCellsTruth(unittest.TestCase):
    def test_small_oneshot_ok(self) -> None:
        errs = check.assert_changed_cells_truthful("s", 1, 49860, "oneshot", 1)
        self.assertEqual(errs, [])

    def test_medium_ok(self) -> None:
        errs = check.assert_changed_cells_truthful("m", 64, 1980, "oneshot", 1)
        self.assertEqual(errs, [])

    def test_large_chunked_ok(self) -> None:
        errs = check.assert_changed_cells_truthful(
            "l", 1200, 49860, "begin_chunk_commit", 3
        )
        self.assertEqual(errs, [])

    def test_rejects_catalog_as_changed(self) -> None:
        errs = check.assert_changed_cells_truthful("bad", 49860, 49860, "oneshot", 1)
        self.assertTrue(any("catalog_cells" in e for e in errs))

    def test_rejects_wrong_transport(self) -> None:
        errs = check.assert_changed_cells_truthful("bad", 1200, 5000, "oneshot", 1)
        self.assertTrue(any("transport" in e for e in errs))


class ReportSchema(unittest.TestCase):
    def _minimal_valid(self) -> dict:
        stroke = {
            "small": {
                "changed_cells": 1,
                "transport": "oneshot",
                "chunks": 1,
                "p95": 1.0,
            },
            "medium": {
                "changed_cells": 64,
                "transport": "oneshot",
                "chunks": 1,
                "p95": 2.0,
            },
            "large": {
                "changed_cells": 1200,
                "transport": "begin_chunk_commit",
                "chunks": 3,
                "p95": 10.0,
            },
        }
        sizes = []
        matrix = []
        for name, cells in (
            ("approx_2k", 1980),
            ("approx_10k", 11968),
            ("approx_25k", 26000),
            ("approx_50k", 49860),
        ):
            sizes.append(
                {
                    "size": name,
                    "catalog_cells": cells,
                    "commit_strokes": stroke,
                    "verdict": {"headless_provisionally_supported": True},
                }
            )
            matrix.append({"size": name, "headless_provisionally_supported": True})
        return {
            "schema": check.SCHEMA,
            "generated_at": "2026-07-18T00:00:00Z",
            "git_sha": "deadbeef",
            "build_mode": "debug-server + web dist",
            "platform": "win32-x64",
            "harness_revision": check.harness_revision(),
            "surface": "playwright-chromium-headless",
            "evidence_class": "reproducible_headless",
            "supported_sot": "owner_windows_tauri_release",
            "headless_verdict": {
                "label": "provisionally_supported_on_headless_benchmark_surface; release_gate_pending",
                "release_gate": "pending",
            },
            "release_gate": {
                "status": "pending",
                "instruction_path": "docs/perf/OWNER-TAURI-RELEASE-GATE.md",
                "owner_run_at": None,
            },
            "memory": {
                "signal": "chromium_js_heap_used_bytes",
                "reliability": "proxy_not_process_rss",
            },
            "operations": {
                "gating": [
                    "open_fit",
                    "pan",
                    "zoom",
                    "stamp_drag",
                    "airbrush_5",
                    "commit_medium",
                ],
                "non_gating_measured": {
                    "view_empty": {"role": "rebuild"},
                    "relief": {"role": "rebuild"},
                },
            },
            "note_facts": "f",
            "note_assumptions": "a",
            "crs_signals": {"viewportCull": True},
            "matrix": matrix,
            "sizes": sizes,
        }

    def test_minimal_valid_passes(self) -> None:
        self.assertEqual(check.validate_report(self._minimal_valid()), [])

    def test_rejects_v1_schema(self) -> None:
        data = self._minimal_valid()
        data["schema"] = "mapkeeper.relief-render-scale.v1"
        errs = check.validate_report(data)
        self.assertTrue(any("schema" in e for e in errs))

    def test_rejects_headless_claiming_release_passed(self) -> None:
        data = self._minimal_valid()
        data["headless_verdict"]["release_gate"] = "passed"
        errs = check.validate_report(data)
        self.assertTrue(any("release_gate" in e for e in errs))

    def test_rejects_misleading_commit_cells(self) -> None:
        data = self._minimal_valid()
        data["sizes"][0]["commit_strokes"]["medium"]["changed_cells"] = 1980
        data["sizes"][0]["commit_strokes"]["medium"]["transport"] = "begin_chunk_commit"
        data["sizes"][0]["commit_strokes"]["medium"]["chunks"] = 4
        errs = check.validate_report(data)
        self.assertTrue(any("catalog_cells" in e for e in errs))


class JsLibSmoke(unittest.TestCase):
    def test_node_lib_transport(self) -> None:
        script = """
import { transportForCellCount, assertChangedCellsTruthful, STROKE_SIZES }
  from './bench-render-scale-lib.mjs';
const t = transportForCellCount(STROKE_SIZES.large);
if (t.transport !== 'begin_chunk_commit' || t.chunks !== 3) throw new Error(JSON.stringify(t));
const err = assertChangedCellsTruthful({
  label: 'x', changed_cells: 49860, catalog_cells: 49860, transport: 'oneshot', chunks: 1
});
if (!err.length) throw new Error('expected catalog misuse error');
console.log('ok');
"""
        with tempfile.NamedTemporaryFile(
            "w", suffix=".mjs", dir=ROOT / "scripts", delete=False, encoding="utf-8"
        ) as f:
            f.write(script)
            path = f.name
        try:
            out = subprocess.check_output(
                ["node", path], cwd=str(ROOT / "scripts"), text=True
            )
            self.assertIn("ok", out)
        finally:
            Path(path).unlink(missing_ok=True)


if __name__ == "__main__":
    unittest.main()
