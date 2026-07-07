#!/usr/bin/env python3
"""Validate example fixtures against the mapkeeper JSON Schemas.

Covers:
- profiles     -> schemas/cell-profile.schema.json     (roadmap 1.2, D-22)
- layers-dense -> schemas/map-layer-dense.schema.json   (scale-layers, D-46)
- manifests    -> schemas/map-manifest.schema.json      (D-36)
- worlds/*/map -> manifest + elevation dense layer      (river-dogfood-fixture-worlds)

For each contract, `<dir>/valid/*.json` must pass its schema and
`<dir>/invalid/*.json` must fail it.

`worlds/<slug>/map/` — every subfolder except `README.md` and `*.py`.

Usage: python fixtures/validate_schema.py
Exit code 0 = every fixture behaved as its folder name promises.
"""
import json
import sys
from pathlib import Path

try:
    import jsonschema
except ImportError:
    print("error: pip install jsonschema", file=sys.stderr)
    sys.exit(2)

ROOT = Path(__file__).resolve().parent.parent
FIXTURES_DIR = Path(__file__).resolve().parent

# (fixtures subdir, schema file) — one entry per data contract.
CONTRACTS = [
    ("profiles", "cell-profile.schema.json"),
    ("layers-dense", "map-layer-dense.schema.json"),
    ("manifests", "map-manifest.schema.json"),
]


def load_schema(name):
    with open(ROOT / "schemas" / name, encoding="utf-8") as f:
        return json.load(f)


def check_dir(schema, dir_path, expect_valid):
    files = sorted(dir_path.glob("*.json"))
    if not files:
        print(f"error: no fixture files in {dir_path}", file=sys.stderr)
        return False
    ok = True
    for path in files:
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        errors = sorted(jsonschema.Draft7Validator(schema).iter_errors(data), key=str)
        is_valid = not errors
        if is_valid == expect_valid:
            print(f"ok   {path.relative_to(ROOT)}")
        else:
            ok = False
            status = "should be valid but failed" if expect_valid else "should be invalid but passed"
            print(f"FAIL {path.relative_to(ROOT)} — {status}", file=sys.stderr)
            for err in errors:
                print(f"       {err.message}", file=sys.stderr)
    return ok


def check_world_fixtures(manifest_schema, layer_schema) -> bool:
    worlds_dir = FIXTURES_DIR / "worlds"
    if not worlds_dir.is_dir():
        return True
    ok = True
    for world_dir in sorted(worlds_dir.iterdir()):
        if not world_dir.is_dir():
            continue
        manifest_path = world_dir / "map" / "manifest.json"
        elev_path = world_dir / "map" / "layers" / "elevation.json"
        if not manifest_path.is_file() or not elev_path.is_file():
            print(f"error: incomplete world fixture {world_dir.name}", file=sys.stderr)
            ok = False
            continue
        for path, schema in (
            (manifest_path, manifest_schema),
            (elev_path, layer_schema),
        ):
            with open(path, encoding="utf-8") as f:
                data = json.load(f)
            errors = sorted(jsonschema.Draft7Validator(schema).iter_errors(data), key=str)
            if not errors:
                print(f"ok   {path.relative_to(ROOT)}")
            else:
                ok = False
                print(f"FAIL {path.relative_to(ROOT)} — should be valid", file=sys.stderr)
                for err in errors:
                    print(f"       {err.message}", file=sys.stderr)
    return ok


def main():
    all_ok = True
    for subdir, schema_name in CONTRACTS:
        schema = load_schema(schema_name)
        base = FIXTURES_DIR / subdir
        valid_ok = check_dir(schema, base / "valid", expect_valid=True)
        invalid_ok = check_dir(schema, base / "invalid", expect_valid=False)
        all_ok = all_ok and valid_ok and invalid_ok
    manifest_schema = load_schema("map-manifest.schema.json")
    layer_schema = load_schema("map-layer-dense.schema.json")
    all_ok = all_ok and check_world_fixtures(manifest_schema, layer_schema)
    if all_ok:
        print("All fixtures matched their expected schema outcome.")
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
