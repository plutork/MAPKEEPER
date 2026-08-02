#!/usr/bin/env python3
"""CI structural gate for N-027 web shell ES modules (not runtime behavior)."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WEB = ROOT / "crates" / "web"

REQUIRED = (
    "index.html",
    "styles.css",
    "main.js",
    "api.js",
    "wasm-api.js",
    "workspace-state.js",
    "camera.js",
    "renderer.js",
    "relief-tool.js",
    "spatial-transaction.js",
    "worlds.js",
    "shell-math.js",
    "brush-geometry.js",
    "hover-readout.js",
    "bench-hooks.js",
)


def main() -> int:
    errors: list[str] = []
    for name in REQUIRED:
        if not (WEB / name).is_file():
            errors.append(f"missing {name}")

    index = (WEB / "index.html").read_text(encoding="utf-8") if (WEB / "index.html").is_file() else ""
    if "<style" in index.lower():
        errors.append("index.html still contains inline <style> (should be styles.css)")
    if "drawSpatial" in index or "beginPaintStroke" in index:
        errors.append("index.html still contains application logic tokens")
    if 'src="./main.js"' not in index and "src='./main.js'" not in index:
        errors.append("index.html must load ./main.js as module entry")
    if 'href="./styles.css"' not in index and "href='./styles.css'" not in index:
        errors.append("index.html must link ./styles.css")

    state = (WEB / "workspace-state.js").read_text(encoding="utf-8") if (WEB / "workspace-state.js").is_file() else ""
    if "export const state" not in state:
        errors.append("workspace-state.js must export sole `state` owner")

    build = (WEB / "build.ps1").read_text(encoding="utf-8") if (WEB / "build.ps1").is_file() else ""
    for asset in (
        "main.js",
        "renderer.js",
        "styles.css",
        "shell-math.js",
        "brush-geometry.js",
        "hover-readout.js",
        "bench-hooks.js",
    ):
        if asset not in build:
            errors.append(f"build.ps1 must stage {asset}")

    # Line budget: shell document stays thin.
    index_lines = index.count("\n") + (1 if index and not index.endswith("\n") else 0)
    if index_lines > 250:
        errors.append(f"index.html too large ({index_lines} lines); expected thin shell document")

    if errors:
        print("check_web_shell_modules: FAIL")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("check_web_shell_modules: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
