#!/usr/bin/env python3
"""
Drift guard for agent operational contract (agent-reliability ops-invariants-doc).

Checks (machine-verifiable only):
- schemas/agent_ops_registry.json parses
- Registry mutating routes match axum routes in server route modules
- Registry operation_id matches op_log classify_route for sample paths
- doc_references paths exist in repo
- docs/OPS-INVARIANTS.md generated section matches gen_ops_invariants.py
- Manual prose markers present (not byte-compared)
"""

from __future__ import annotations

import importlib.util
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY_PATH = ROOT / "schemas" / "agent_ops_registry.json"
DOC = ROOT / "docs" / "OPS-INVARIANTS.md"
BEGIN = "<!-- GENERATED:BEGIN -->"
END = "<!-- GENERATED:END -->"

ROUTE_MODULES = [
    ROOT / "crates/server/src/projects.rs",
    ROOT / "crates/server/src/build.rs",
    ROOT / "crates/server/src/layers.rs",
    ROOT / "crates/server/src/rivers.rs",
    ROOT / "crates/server/src/lakes.rs",
]

MUTATE_METHODS = {"POST", "PUT", "DELETE", "PATCH"}

PATH_IN_BACKTICKS = re.compile(
    r"`((?:crates|docs|schemas|toolchain|fixtures|\.github)/[^`\s]+)`"
)


def rel(path: Path) -> str:
    return path.relative_to(ROOT).as_posix()


def load_registry() -> dict:
    return json.loads(REGISTRY_PATH.read_text(encoding="utf-8"))


def classify_route(path: str) -> str:
    """Mirror crates/server/src/op_log.rs::classify_route (keep in sync)."""
    if path == "/api/projects":
        return "projects"
    if path == "/api/projects/open":
        return "projects.open"
    if path == "/api/projects/forget":
        return "projects.forget"
    if path == "/api/projects/delete":
        return "projects.delete"
    if path == "/api/projects/close":
        return "projects.close"
    if path == "/api/fixture-worlds/open":
        return "fixture_worlds.open"
    if path == "/api/build":
        return "build.state"
    if path == "/api/build/bounds":
        return "build.bounds"
    if path == "/api/build/land-mask/generate":
        return "build.land_mask.generate"
    if path == "/api/build/land-mask/cells":
        return "build.land_mask.cells"
    if path == "/api/build/geology/generate":
        return "build.geology.generate"
    if path == "/api/build/elevation/generate":
        return "build.elevation.generate"
    if path == "/api/build/climate/generate":
        return "build.climate.generate"
    if path == "/api/lakes":
        return "lakes"
    if path == "/api/lakes/generate":
        return "lakes.generate"
    if path == "/api/rivers":
        return "rivers"
    if path == "/api/rivers/pin":
        return "rivers.pin"
    if path == "/api/rivers/append":
        return "rivers.append"
    if path == "/api/rivers/generate":
        return "rivers.generate"
    if path.startswith("/api/cells/") and path.endswith("/profile"):
        return "cells.profile"
    if path.startswith("/api/layers/") and path.endswith("/batch"):
        return "layers.batch"
    if path.startswith("/api/layers/") and "/cells/" in path:
        return "layers.cell"
    if path.startswith("/api/rivers/") and path.endswith("/detach"):
        return "rivers.detach"
    if path.startswith("/api/rivers/") and path.endswith("/pop"):
        return "rivers.pop"
    if path.startswith("/api/rivers/"):
        return "rivers.id"
    return "unknown"


def operation_kind(method: str, path: str) -> str:
    return f"{method.lower()}.{classify_route(path)}"


def scan_mutating_routes() -> set[tuple[str, str]]:
    """Extract (METHOD, path) mutating routes from server route modules."""
    found: set[tuple[str, str]] = set()
    path_re = re.compile(r'["\'](/api/[^"\']+)["\']')
    method_patterns = [
        (re.compile(r"(?:^|[^\w])get\s*\("), "GET"),
        (re.compile(r"(?:^|[^\w])put\s*\("), "PUT"),
        (re.compile(r"(?:^|[^\w])post\s*\("), "POST"),
        (re.compile(r"(?:^|[^\w])delete\s*\("), "DELETE"),
        (re.compile(r"(?:^|[^\w])patch\s*\("), "PATCH"),
        (re.compile(r"axum::routing::put\s*\("), "PUT"),
        (re.compile(r"axum::routing::post\s*\("), "POST"),
        (re.compile(r"axum::routing::delete\s*\("), "DELETE"),
        (re.compile(r"axum::routing::patch\s*\("), "PATCH"),
    ]

    for module in ROUTE_MODULES:
        text = module.read_text(encoding="utf-8")
        # Split on `.route(` chunks so method tokens stay inside the same route call.
        chunks = text.split(".route(")
        for chunk in chunks[1:]:
            path_match = path_re.search(chunk)
            if not path_match:
                continue
            path = path_match.group(1)
            depth = 1
            handler_src = []
            for ch in chunk[path_match.end() :]:
                if ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
                    if depth == 0:
                        break
                handler_src.append(ch)
            handler = "".join(handler_src)
            for regex, method in method_patterns:
                if regex.search(handler) and method in MUTATE_METHODS:
                    found.add((method, path))
    return found


def registry_mutating_routes(registry: dict) -> set[tuple[str, str]]:
    out: set[tuple[str, str]] = set()
    for op in registry["operations"]:
        out.add((op["method"].upper(), op["path"]))
    return out


def check_route_parity(registry: dict) -> list[str]:
    errors: list[str] = []
    scanned = scan_mutating_routes()
    declared = registry_mutating_routes(registry)
    missing = scanned - declared
    stale = declared - scanned
    for method, path in sorted(missing):
        errors.append(f"registry missing mutating route {method} {path} (found in server)")
    for method, path in sorted(stale):
        errors.append(f"registry lists unknown route {method} {path} (not in server modules)")
    return errors


def check_operation_ids(registry: dict) -> list[str]:
    errors: list[str] = []
    for op in registry["operations"]:
        expected = operation_kind(op["method"], op["path"])
        if op["operation_id"] != expected:
            errors.append(
                f"operation_id mismatch for {op['method']} {op['path']}: "
                f"registry `{op['operation_id']}` vs op_log `{expected}`"
            )
    return errors


def check_doc_references(registry: dict) -> list[str]:
    errors: list[str] = []
    for raw in registry.get("doc_references", []):
        path = ROOT / raw
        if not path.exists():
            errors.append(f"doc_references missing path `{raw}`")
    return errors


def check_doc_paths_in_manual() -> list[str]:
    errors: list[str] = []
    if not DOC.is_file():
        errors.append(f"missing {rel(DOC)}")
        return errors
    text = DOC.read_text(encoding="utf-8")
    manual = text.split(BEGIN, 1)[0] if BEGIN in text else text
    for match in PATH_IN_BACKTICKS.finditer(manual):
        raw = match.group(1).rstrip("/")
        if raw.endswith("..."):
            continue
        if "*" in raw or "{" in raw:
            continue
        path = ROOT / raw
        if not path.exists():
            errors.append(f"{rel(DOC)} manual section references missing path `{raw}`")
    return errors


def check_generated_fresh(registry: dict) -> list[str]:
    errors: list[str] = []
    if not DOC.is_file():
        errors.append(f"missing {rel(DOC)} — run: python scripts/gen_ops_invariants.py")
        return errors
    text = DOC.read_text(encoding="utf-8")
    if BEGIN not in text or END not in text:
        errors.append(f"{rel(DOC)} missing generated markers")
        return errors

    spec = importlib.util.spec_from_file_location(
        "gen_ops_invariants", ROOT / "scripts" / "gen_ops_invariants.py"
    )
    mod = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    sys.modules["gen_ops_invariants"] = mod
    spec.loader.exec_module(mod)

    expected_block = f"{BEGIN}\n{mod.render_generated(registry)}\n{END}"
    if expected_block not in text:
        errors.append(
            f"{rel(DOC)} generated section stale — run: python scripts/gen_ops_invariants.py"
        )
    return errors


def check_required_manual_sections() -> list[str]:
    errors: list[str] = []
    if not DOC.is_file():
        return errors
    text = DOC.read_text(encoding="utf-8").lower()
    required = [
        "safe sequences",
        "recovery",
        "authoritative",
        "derived",
    ]
    for heading in required:
        if heading not in text:
            errors.append(f"{rel(DOC)} missing manual section keyword `{heading}`")
    return errors


def main() -> int:
    errors: list[str] = []
    if not REGISTRY_PATH.is_file():
        print(f"missing {rel(REGISTRY_PATH)}", file=sys.stderr)
        return 1

    registry = load_registry()
    errors.extend(check_route_parity(registry))
    errors.extend(check_operation_ids(registry))
    errors.extend(check_doc_references(registry))
    errors.extend(check_doc_paths_in_manual())
    errors.extend(check_generated_fresh(registry))
    errors.extend(check_required_manual_sections())

    if errors:
        print("OPS-INVARIANTS drift detected:", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        return 1

    print(f"OK ops invariants drift check ({rel(REGISTRY_PATH)}, {rel(DOC)})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
