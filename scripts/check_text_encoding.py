#!/usr/bin/env python3
"""Fail CI on CP1252 mojibake in sources and non-ASCII console output in alpha .ps1."""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

# UTF-8 Cyrillic misread as CP1252 (web .rs regression class).
MOJIBAKE_FRAGMENTS = (
    "тАж",
    "тАФ",
    "тАУ",
    "тА║",
    "тАЬ",
    "тАЭ",
    "┬╖",
    "├Ч",
    "тЖТ",
    "тЖР",
    "тЦ╝",
    "тЦ╢",
    "тЙИ",
    "вЂ",
)

# Alpha scripts run in Windows PowerShell 5.x default code page — keep console text ASCII.
PS1_CONSOLE = (
    ROOT / "run.ps1",
    ROOT / "setup.ps1",
    ROOT / "update.ps1",
    ROOT / "crates" / "web" / "build.ps1",
    ROOT / "scripts" / "check.ps1",
    ROOT / "scripts" / "smoke-headless.ps1",
)

WEB_SRC = ROOT / "crates" / "web" / "src"


def scan_mojibake(path: Path) -> list[str]:
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError as exc:
        return [f"{path}: not valid UTF-8 ({exc})"]
    hits = [frag for frag in MOJIBAKE_FRAGMENTS if frag in text]
    if hits:
        return [f"{path}: mojibake fragments: {', '.join(hits)}"]
    return []


def scan_ps1_ascii(path: Path) -> list[str]:
    text = path.read_text(encoding="utf-8")
    bad = [ch for ch in text if ord(ch) > 127]
    if bad:
        sample = "".join(sorted(set(bad)))
        return [f"{path}: non-ASCII in alpha script (use ASCII for console): {sample!r}"]
    return []


def main() -> int:
    errors: list[str] = []

    if WEB_SRC.is_dir():
        for path in sorted(WEB_SRC.rglob("*.rs")):
            errors.extend(scan_mojibake(path))

    for path in PS1_CONSOLE:
        if path.is_file():
            errors.extend(scan_mojibake(path))
            errors.extend(scan_ps1_ascii(path))

    if errors:
        print("text encoding check failed:", file=sys.stderr)
        for line in errors:
            print(f"  {line}", file=sys.stderr)
        return 1

    print("text encoding check OK")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
