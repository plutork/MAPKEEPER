#!/usr/bin/env python3
"""Render generated sections of docs/OPS-INVARIANTS.md from schemas/agent_ops_registry.json."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
REGISTRY = ROOT / "schemas" / "agent_ops_registry.json"
DOC = ROOT / "docs" / "OPS-INVARIANTS.md"

BEGIN = "<!-- GENERATED:BEGIN -->"
END = "<!-- GENERATED:END -->"


def load_registry() -> dict:
    return json.loads(REGISTRY.read_text(encoding="utf-8"))


def _join(items: list[str] | None) -> str:
    if not items:
        return "—"
    return ", ".join(items)


def render_generated(registry: dict) -> str:
    lines: list[str] = []
    lines.append("## Generated reference (do not edit by hand)")
    lines.append("")
    lines.append(
        f"_Source: `{REGISTRY.relative_to(ROOT).as_posix()}` · "
        f"regenerate: `python scripts/gen_ops_invariants.py`_"
    )
    lines.append("")

    lines.append("### Artifacts")
    lines.append("")
    lines.append("| Kind | ID | Path | Role |")
    lines.append("|------|-----|------|------|")
    for row in registry["artifacts"]["authoritative"]:
        lines.append(f"| authoritative | {row['id']} | `{row['path']}` | {row['role']} |")
    for row in registry["artifacts"]["derived"]:
        src = row.get("source", "—")
        lines.append(
            f"| derived | {row['id']} | `{row['path']}` | from {src}; {row['role']} |"
        )
    for row in registry["artifacts"]["server_registry"]:
        lines.append(f"| server | {row['id']} | `{row['path']}` | {row['role']} |")
    lines.append("")

    lines.append("### Operation bundles")
    lines.append("")
    lines.append("| Bundle | Txn | Writes | Deletes | Invalidates |")
    lines.append("|--------|-----|--------|---------|-------------|")
    for b in registry["bundles"]:
        lines.append(
            f"| `{b['id']}` | {b.get('txn', '—')} | {_join(b.get('writes'))} | "
            f"{_join(b.get('deletes'))} | {_join(b.get('invalidates'))} |"
        )
    lines.append("")

    lines.append("### Invalidation graph")
    lines.append("")
    lines.append("| Trigger | When | Invalidates |")
    lines.append("|---------|------|-------------|")
    for edge in registry["invalidation_graph"]:
        when = edge.get("when", "always")
        targets = ", ".join(f"`{t}`" for t in edge["invalidates"])
        lines.append(f"| `{edge['trigger']}` | {when} | {targets} |")
    lines.append("")

    lines.append("### Mutating operations")
    lines.append("")
    lines.append(
        "| Method | Path | operation_id | Scope | base_revision | Bundle | "
        "Reads | Writes | Invalidates | Conflicts |"
    )
    lines.append(
        "|--------|------|----------------|-------|---------------|--------|"
        "-------|--------|-------------|-----------|"
    )
    for op in registry["operations"]:
        lines.append(
            f"| {op['method']} | `{op['path']}` | `{op['operation_id']}` | "
            f"{op['scope']} | {op['base_revision']} | `{op['bundle']}` | "
            f"{_join(op.get('reads'))} | {_join(op.get('writes'))} | "
            f"{_join(op.get('invalidates'))} | {_join(op.get('conflicts_with'))} |"
        )
    lines.append("")

    lines.append("### Conflict groups")
    lines.append("")
    for g in registry["conflict_groups"]:
        lines.append(f"- **`{g['id']}`** ({g['scope']}): {g['rule']}")
    lines.append("")

    return "\n".join(lines)


def render_doc(registry: dict) -> str:
    prose = DOC.read_text(encoding="utf-8") if DOC.is_file() else ""
    if BEGIN not in prose or END not in prose:
        raise SystemExit(f"{DOC} missing {BEGIN} / {END} markers")
    head, rest = prose.split(BEGIN, 1)
    _, tail = rest.split(END, 1)
    generated = render_generated(registry)
    return f"{head.rstrip()}\n\n{BEGIN}\n{generated}\n{END}{tail}"


def main() -> int:
    registry = load_registry()
    DOC.write_text(render_doc(registry), encoding="utf-8", newline="\n")
    print(f"Wrote {DOC.relative_to(ROOT).as_posix()}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
