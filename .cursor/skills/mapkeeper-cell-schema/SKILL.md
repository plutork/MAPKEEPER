---
name: mapkeeper-cell-schema
description: >-
  JSON Schema and profile JSON for mapkeeper hex cells — cell_id format, V0
  fields, validation posture. Use when editing schemas/, profile examples, or
  discussing cell profile fields (roadmap 3.2).
paths: schemas/**, **/profiles/**
---

# Cell profile schema (mapkeeper V0)

**Product invariants:** `README.md#invariants` · decided scope: D-12 (maintainer decisions log).

## cell_id (canonical)

```
{world_id}.hex.q{q}.r{r}
```

Example file: `profiles/{world_id}.hex.q3.r-1.json`

## V0 field guidance *(Shape may refine via /idea)*

| Field | Role |
|-------|------|
| `cell_id` | canonical key — matches filename stem |
| `display_name` | human label for author/GM |
| `slug` | optional short id for agents — not primary key |
| `notes` | free text stub for V0 |

Keep schema **minimal** — writer/GM persona; no dev jargon in author-facing descriptions.

## Validation posture *(D-23, final for V0)*

**Save never blocks** — neither web UI nor CLI. Empty `display_name` is a
**valid** state ("painted, unnamed yet"), not a warning. Malformed
`cell_id` stays a defensive `Error` in `CellProfile::validate()` (for
future hand-edited files / import) but is unreachable via real entry
points today — that is expected, not a bug. No format rules for
`slug`/`notes`. CI schema tests against this file — still open, Later,
tracked separately (roadmap 1.2 V0-done criterion), not part of this
decision.

## Files

- Schema: `schemas/` in this repo (layout fixed — D-20 Cargo workspace).
- Rust model: `crates/core/src/profile.rs` (`CellProfile`) — schema here is the source of truth; keep the Rust struct in sync.
- World instances: author world repo `profiles/` — not in product repo.

## Gate

Maintainer edits here only under approved build scope (`/real` with backing D-* / todo).
