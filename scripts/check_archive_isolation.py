#!/usr/bin/env python3
"""Ensure active build inputs do not depend on the map-v2 archive."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ARCHIVE = ROOT / "archive" / "map-v2"
ACTIVE_PATTERNS = (
    "crates/*/Cargo.toml",
    "crates/*/src/**/*.rs",
    "crates/web/build.ps1",
    "crates/desktop/tauri.conf.json",
)
FORBIDDEN_ACTIVE = (
    "crates/cli",
    "crates/core/src/worldgen",
    "crates/core/src/hex.rs",
    "crates/core/src/layer.rs",
    "crates/core/src/history.rs",
    "crates/server/src/build.rs",
    "crates/server/src/layers.rs",
    "crates/server/src/rivers.rs",
    "crates/server/src/lakes.rs",
    "crates/web/src/canvas.rs",
    "crates/web/src/wizard.rs",
    "crates/web/src/history.rs",
    "schemas",
    "fixtures",
    "toolchain/template/world",
    "tests/smoke.mjs",
    "docs/WORLD-GENERATION-PIPELINE.md",
    "docs/WORLD-GEN-UI.md",
    "docs/OPS-INVARIANTS.md",
)
FORBIDDEN_CODE_TOKENS = (
    "MapBounds",
    "DenseLayer",
    "cell_id",
    "land_mask",
    "RiverCatalog",
    "LakeCatalog",
    "worldgen",
)


def main() -> int:
    errors: list[str] = []
    if not (ARCHIVE / "README.md").is_file() or not (ARCHIVE / "RETIREMENT.md").is_file():
        errors.append("archive/map-v2 requires README.md and RETIREMENT.md")
    root_manifest = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    if 'exclude = ["archive/map-v2"]' not in root_manifest:
        errors.append("Cargo workspace must exclude archive/map-v2")
    for relative in FORBIDDEN_ACTIVE:
        if (ROOT / relative).exists():
            errors.append(f"map-v2 artifact remains active: {relative}")
    for pattern in ACTIVE_PATTERNS:
        for path in ROOT.glob(pattern):
            if not path.is_file():
                continue
            text = path.read_text(encoding="utf-8")
            if "archive/map-v2" in text or "archive\\map-v2" in text:
                errors.append(f"active build input references archive: {path.relative_to(ROOT)}")
            for token in FORBIDDEN_CODE_TOKENS:
                if token in text:
                    errors.append(
                        f"active build input retains map-v2 token `{token}`: {path.relative_to(ROOT)}"
                    )
    if errors:
        print("Archive isolation failed:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("archive/map-v2 isolation OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
