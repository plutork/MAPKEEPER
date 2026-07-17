#!/usr/bin/env python3
"""Fail when public docs / smoke still describe the pre-spatial identity-only shell."""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# Paths that must describe the active spatial product (not archived reset prose).
ACTIVE_PATHS = (
    ROOT / "docs" / "DEV.md",
    ROOT / "docs" / "CURSOR-ALPHA.md",
    ROOT / "README.md",
    ROOT / "AGENTS.md",
    ROOT / "docs" / "CODEMAP-LITE.md",
    ROOT / "scripts" / "smoke-headless.ps1",
    ROOT / ".github" / "workflows" / "ci.yml",
)

# Forbidden in active product guidance (archive/ may keep historical wording).
FORBIDDEN = (
    (
        re.compile(r"identity-only\s+worlds?", re.I),
        "stale 'identity-only world(s)' — product has spatial config + state",
    ),
    (
        re.compile(
            r"limited to `/api/health` and `/api/projects`|"
            r"health/projects\s+API\s*\+|"
            r"active API is\s+limited",
            re.I,
        ),
        "stale health/projects-only API description",
    ),
    (
        re.compile(r"карта/schemas\s+ещё\s+не\s+приняты", re.I),
        "stale Russian 'map/schemas not accepted' copy",
    ),
)


def main() -> int:
    errors: list[str] = []
    for path in ACTIVE_PATHS:
        if not path.is_file():
            errors.append(f"missing {path.relative_to(ROOT).as_posix()}")
            continue
        text = path.read_text(encoding="utf-8", errors="replace")
        rel = path.relative_to(ROOT).as_posix()
        for pat, why in FORBIDDEN:
            if pat.search(text):
                errors.append(f"{rel}: {why}")
    if errors:
        print("check_doc_drift: FAIL")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("check_doc_drift: OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
