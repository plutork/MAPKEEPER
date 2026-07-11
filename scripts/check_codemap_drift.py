#!/usr/bin/env python3
"""
CI drift guard for MAPKEEPER codemap docs (layer 3, todo codemap-auto-refresh-layer-3).

- CODEMAP.md must match `scripts/gen_codemap.py` output (regenerate locally, commit).
- CODEMAP-LITE.md must not reference removed top-level worldgen paths (D-92) or missing files.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
LITE = ROOT / "docs" / "CODEMAP-LITE.md"
GENERATED = ROOT / "docs" / "CODEMAP.md"

# Top-level core files removed by D-92 — must not appear as primary routes in lite docs.
STALE_TOP_LEVEL = (
    "crates/core/src/geology.rs",
    "crates/core/src/land_mask.rs",
    "crates/core/src/elevation_gen.rs",
    "crates/core/src/climate.rs",
    "crates/core/src/river_flux.rs",
    "crates/core/src/coast_distance.rs",
    "crates/core/src/plates.rs",
)

# D-96: server lib.rs is facade-only — these must route to extracted modules, not lib.rs.
STALE_SERVER_LIB_MARKERS = (
    "write_map_manifest",
    "write_dense_layer",
    "read_or_empty",
    "/api/build/",
    "/api/layers/",
    "/api/rivers",
    "/api/map",
    "/api/projects",
    "generate_elevation",
    "generate_climate",
    "generate_rivers",
)

PATH_IN_BACKTICKS = re.compile(
    r"`((?:crates|docs|schemas|toolchain|fixtures|\.github)/[^`\s]+)`"
)


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def check_generated_fresh() -> list[str]:
    errors: list[str] = []
    if not GENERATED.is_file():
        errors.append(f"missing {rel(GENERATED)} — run: python scripts/gen_codemap.py")
        return errors

    # Import render from sibling script.
    import importlib.util

    spec = importlib.util.spec_from_file_location(
        "gen_codemap", ROOT / "scripts" / "gen_codemap.py"
    )
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules["gen_codemap"] = mod
    spec.loader.exec_module(mod)

    expected = mod.render()
    actual = GENERATED.read_text(encoding="utf-8")
    if expected != actual:
        errors.append(
            f"{rel(GENERATED)} is stale — run: python scripts/gen_codemap.py && git add docs/CODEMAP.md"
        )
    return errors


def check_lite_paths() -> list[str]:
    errors: list[str] = []
    if not LITE.is_file():
        errors.append(f"missing {rel(LITE)}")
        return errors

    text = LITE.read_text(encoding="utf-8")

    for stale in STALE_TOP_LEVEL:
        if stale in text:
            errors.append(f"{rel(LITE)} references removed path `{stale}` (use worldgen/ or legacy note)")

    for line in text.splitlines():
        if "crates/server/src/lib.rs" not in line:
            continue
        for marker in STALE_SERVER_LIB_MARKERS:
            if marker in line:
                errors.append(
                    f"{rel(LITE)} routes `{marker}` to `crates/server/src/lib.rs` "
                    f"(D-96 facade — use build/world_io/layers/projects/rivers)"
                )
                break

    for match in PATH_IN_BACKTICKS.finditer(text):
        raw = match.group(1).rstrip("/")
        if raw.endswith("..."):
            continue
        if "*" in raw or "<" in raw:
            continue
        path = ROOT / raw
        if not path.exists():
            errors.append(f"{rel(LITE)} references missing path `{raw}`")

    worldgen = ROOT / "crates" / "core" / "src" / "worldgen"
    if worldgen.is_dir() and "worldgen/" not in text and "worldgen\\" not in text:
        errors.append(f"{rel(LITE)} should mention `worldgen/` (core pipeline exists)")

    return errors


def main() -> int:
    errors = check_generated_fresh() + check_lite_paths()
    if errors:
        print("Codemap drift detected:", file=sys.stderr)
        for e in errors:
            print(f"  - {e}", file=sys.stderr)
        return 1
    print(f"OK codemap drift check ({rel(LITE)}, {rel(GENERATED)})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
