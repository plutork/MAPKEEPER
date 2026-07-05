#!/usr/bin/env python3
"""Validate fixtures/profiles/{valid,invalid}/*.json against
schemas/cell-profile.schema.json (roadmap 1.2 V0-done criterion, D-22).

Usage: python fixtures/validate_schema.py
Exit code 0 = all fixtures behaved as their folder name promises.
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
SCHEMA_PATH = ROOT / "schemas" / "cell-profile.schema.json"
FIXTURES_DIR = Path(__file__).resolve().parent / "profiles"


def load_schema():
    with open(SCHEMA_PATH, encoding="utf-8") as f:
        return json.load(f)


def check_dir(schema, dir_name, expect_valid):
    dir_path = FIXTURES_DIR / dir_name
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


def main():
    schema = load_schema()
    valid_ok = check_dir(schema, "valid", expect_valid=True)
    invalid_ok = check_dir(schema, "invalid", expect_valid=False)
    if valid_ok and invalid_ok:
        print("All fixtures matched their expected schema outcome.")
        return 0
    return 1


if __name__ == "__main__":
    sys.exit(main())
