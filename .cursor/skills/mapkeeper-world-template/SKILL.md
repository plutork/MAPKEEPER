---
name: mapkeeper-world-template
description: >-
  Author world template bundle — toolchain/template/world, GitHub template
  sync, /user command in bundle. Use when editing world scaffold, template
  sync, or author onboarding files.
paths: toolchain/template/world/**, toolchain/template/sync-template.ps1
---

# World template (mapkeeper)

**Decisions:** D-08 GitHub Template · D-10 sync CI · D-12 wizard replaces Template for V0-done persona.

## Source of truth

Edit **`toolchain/template/world/`** only — not manual copy from legacy `toolchain/cursor/`.

`/user` ships **inside** template bundle at `.cursor/commands/user.md`.

## Sync (D-10)

- Local optional: `toolchain/template/sync-template.ps1`
- CI: `.github/workflows/sync-world-template.yml` → `mapkeeper-world-template`
- Agents: edit `world/` → commit → CI mirrors; no manual robocopy workflow.

## Author layout *(world repo)*

`mapkeeper.toml`, `map/`, `canon/`, `profiles/`, `data/`, `journal/`, `.cursor/commands/user.md`, `AGENTS.md`

## Public MAPKEEPER rules

- No private maintainer text in the public kit.
- Template is **interim** onboarding until editor wizard (D-09).

## Gate

Template changes are product work — only through approved `/real` scope with D-* backing.
