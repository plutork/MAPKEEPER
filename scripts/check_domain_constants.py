#!/usr/bin/env python3
"""CI parity gate for N-030: shell copies of domain thresholds must not drift.

A threshold may be mirrored in the web shell for rendering or transport, but the
domain crate stays the source. This script fails when a mirror disagrees with
its Rust constant, and when a domain rule reappears as a second formula in JS.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
FIELD_RS = ROOT / "crates" / "core" / "src" / "spatial" / "field.rs"
BRUSH_RS = ROOT / "crates" / "core" / "src" / "spatial" / "brush.rs"
STATE_JS = ROOT / "crates" / "web" / "workspace-state.js"
SHELL_MATH_JS = ROOT / "crates" / "web" / "shell-math.js"
RELIEF_TOOL_JS = ROOT / "crates" / "web" / "relief-tool.js"

# Mirrored threshold: (label, rust file, rust const, js file, js const).
MIRRORED = (
    ("elevation min", FIELD_RS, "RELIEF_MIN", STATE_JS, "ELEV_MIN"),
    ("elevation max", FIELD_RS, "RELIEF_MAX", STATE_JS, "ELEV_MAX"),
    ("flush batch", BRUSH_RS, "FIELD_FLUSH_BATCH_MAX", STATE_JS, "FIELD_FLUSH_BATCH_MAX"),
)


def rust_const(path: Path, name: str) -> int | None:
    text = path.read_text(encoding="utf-8")
    match = re.search(rf"pub const {name}\s*:\s*\w+\s*=\s*(-?\d+)", text)
    return int(match.group(1)) if match else None


def js_const(path: Path, name: str) -> int | None:
    text = path.read_text(encoding="utf-8")
    match = re.search(rf"export const {name}\s*=\s*(-?\d+)", text)
    return int(match.group(1)) if match else None


def main() -> int:
    errors: list[str] = []

    for label, rust_path, rust_name, js_path, js_name in MIRRORED:
        rust_value = rust_const(rust_path, rust_name)
        js_value = js_const(js_path, js_name)
        if rust_value is None:
            errors.append(f"{label}: {rust_name} not found in {rust_path.name}")
            continue
        if js_value is None:
            errors.append(f"{label}: {js_name} not found in {js_path.name}")
            continue
        if rust_value != js_value:
            errors.append(
                f"{label}: {rust_name}={rust_value} in {rust_path.name} but "
                f"{js_name}={js_value} in {js_path.name}"
            )

    # The gesture rule itself must come from the domain, not a JS reimplementation.
    shell_math = SHELL_MATH_JS.read_text(encoding="utf-8")
    if "editOcean" in shell_math:
        errors.append("shell-math.js reimplements the ocean rule; call probe_next_relief instead")
    relief_tool = RELIEF_TOOL_JS.read_text(encoding="utf-8")
    if "probe_next_relief" not in relief_tool:
        errors.append("relief-tool.js must take the gesture rule from the domain bridge")

    if errors:
        print("check_domain_constants: FAIL")
        for item in errors:
            print(f"  - {item}")
        return 1
    print("check_domain_constants: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
