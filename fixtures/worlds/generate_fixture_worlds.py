#!/usr/bin/env python3
"""Generate Small (14x8) elevation fixture worlds for river dogfood.

Mirrors core::hex::MapBounds row-major index: index = row * width + col.

Usage (repo root): python fixtures/worlds/generate_fixture_worlds.py
"""
from __future__ import annotations

import json
from pathlib import Path

WIDTH = 14
HEIGHT = 8
CELL_COUNT = WIDTH * HEIGHT
OUT = Path(__file__).resolve().parent


def index(col: int, row: int) -> int:
    return row * WIDTH + col


def build_layer(elev_fn) -> dict:
    values: list[int] = []
    states: list[int] = []
    for row in range(HEIGHT):
        for col in range(WIDTH):
            values.append(int(elev_fn(col, row)))
            states.append(2)
    return {
        "schema_version": 2,
        "layer_id": "elevation",
        "value_type": "integer",
        "cell_count": CELL_COUNT,
        "states": states,
        "values": values,
    }


def coastal_slope(col: int, row: int) -> int:
    # Ocean west; land rises east — rivers drain left into sea.
    if col <= 1:
        return 0
    return 6 + (col - 2) * 4


def mountain_ridge(col: int, row: int) -> int:
    # Central ridge; both slopes drain to east/west coasts.
    if col <= 0 or col >= WIDTH - 1:
        return 0
    peak = 52 - abs(col - 7) * 7 - abs(row - (HEIGHT // 2)) * 4
    return max(0, peak)


def enclosed_basin(col: int, row: int) -> int:
    # Low bowl center; ring of hills; ocean rim — depression-fill test Later.
    if col <= 0 or col >= WIDTH - 1 or row <= 0 or row >= HEIGHT - 1:
        return 0
    cx, cy = 6.5, 3.5
    dist = ((col - cx) ** 2 + (row - cy) ** 2) ** 0.5
    if dist < 2.2:
        return 4
    if dist < 4.0:
        return 28
    return 12


def gentle_plain(col: int, row: int) -> int:
    # Mild north→south slope; subtle routing choices.
    base = 14 + row * 3
    wobble = (col % 3) - 1
    return max(1, base + wobble)


def dual_watershed(col: int, row: int) -> int:
    # Two peaks drain to opposite coasts through a central valley.
    if col <= 0 or col >= WIDTH - 1:
        return 0
    left_peak = 40 - (abs(col - 3) * 6 + abs(row - 3) * 3)
    right_peak = 38 - (abs(col - 10) * 6 + abs(row - 4) * 3)
    elev = max(left_peak, right_peak)
    if 5 <= col <= 8 and elev < 12:
        return 8
    return max(0, elev)


WORLDS: dict[str, tuple[str, object]] = {
    "coastal-slope": (
        "West ocean, land rises east — downhill rivers to the sea.",
        coastal_slope,
    ),
    "mountain-ridge": (
        "Central ridge with ocean on both sides — two opposite drainages.",
        mountain_ridge,
    ),
    "enclosed-basin": (
        "Low interior bowl surrounded by hills and ocean rim — depression test.",
        enclosed_basin,
    ),
    "gentle-plain": (
        "Mild north→south slope with small noise — subtle path choices.",
        gentle_plain,
    ),
    "dual-watershed": (
        "Two peaks and central valley — opposite coast drainages.",
        dual_watershed,
    ),
}


def manifest() -> dict:
    return {
        "schema_version": 1,
        "bounds": {"kind": "hex-rectangle", "width": WIDTH, "height": HEIGHT},
        "layers": [
            {
                "layer_id": "elevation",
                "value_type": "integer",
                "file": "layers/elevation.json",
            }
        ],
    }


def mapkeeper_toml(slug: str) -> str:
    wid = f"fixture-{slug}"
    return (
        "# mapkeeper fixture world (river dogfood)\n\n"
        f'[world]\nid = "{wid}"\nname = "{wid}"\nversion = "0.1.0"\n'
    )


def write_world(slug: str, _desc: str, elev_fn) -> None:
    root = OUT / slug
    (root / "map" / "layers").mkdir(parents=True, exist_ok=True)
    with open(root / "mapkeeper.toml", "w", encoding="utf-8") as f:
        f.write(mapkeeper_toml(slug))
    with open(root / "map" / "manifest.json", "w", encoding="utf-8") as f:
        json.dump(manifest(), f, indent=2)
        f.write("\n")
    with open(root / "map" / "layers" / "elevation.json", "w", encoding="utf-8") as f:
        json.dump(build_layer(elev_fn), f, indent=2)
        f.write("\n")
    print(f"wrote {slug}")


def main() -> None:
    for slug, (desc, fn) in WORLDS.items():
        write_world(slug, desc, fn)
    print(f"done: {len(WORLDS)} worlds ({CELL_COUNT} cells each)")


if __name__ == "__main__":
    main()
