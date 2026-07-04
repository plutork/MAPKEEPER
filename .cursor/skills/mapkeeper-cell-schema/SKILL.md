---
name: mapkeeper-cell-schema
description: >-
  JSON Schema and profile JSON for mapkeeper hex cells — cell_id format, V0
  fields, validation posture. Use when editing schemas/, profile examples, or
  discussing cell profile fields (roadmap 3.2).
paths: schemas/**, **/profiles/**
---

# Cell profile schema (mapkeeper V0)

**Product invariants:** `STARTER_PACK.md` · **Decided scope:** D-12 in MAPKEEPER-OS `decisions.md`.

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

## Validation posture *(open 3.4)*

Until decided: prefer **warn on save, block on CI** for schema tests — propose change via MAPKEEPER-OS `/idea` if stricter.

## Files

- Schema: `schemas/` in this repo (layout TBD — agree in `/idea repo layout`).
- World instances: author world repo `profiles/` — not in product repo.

## Gate

Maintainer edits here only under MAPKEEPER-OS **`/real`** with D-* or active `/done` plan.
