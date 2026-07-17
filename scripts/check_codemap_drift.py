#!/usr/bin/env python3
"""Check active shell codemap freshness and documented paths."""

from __future__ import annotations

import importlib.util
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
GENERATED = ROOT / "docs" / "CODEMAP.md"
LITE = ROOT / "docs" / "CODEMAP-LITE.md"
PATH = re.compile(r"`((?:crates|docs|scripts|archive)/[^`\s]+)`")


def main() -> int:
    errors: list[str] = []
    spec = importlib.util.spec_from_file_location("gen_codemap", ROOT / "scripts/gen_codemap.py")
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules["gen_codemap"] = module
    spec.loader.exec_module(module)
    if not GENERATED.is_file() or GENERATED.read_text(encoding="utf-8") != module.render():
        errors.append("docs/CODEMAP.md is stale; run python scripts/gen_codemap.py")
    if not LITE.is_file():
        errors.append("docs/CODEMAP-LITE.md is missing")
    else:
        text = LITE.read_text(encoding="utf-8")
        for match in PATH.finditer(text):
            raw = match.group(1).rstrip("/")
            if "*" not in raw and not (ROOT / raw).exists():
                errors.append(f"docs/CODEMAP-LITE.md references missing path: {raw}")
    if errors:
        print("Codemap drift detected:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1
    print("OK codemap drift check")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
