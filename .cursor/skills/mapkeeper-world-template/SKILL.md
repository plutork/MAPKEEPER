---
name: mapkeeper-world-template
description: >-
  Embedded author-world scaffold — toolchain/template/world for editor/CLI
  (D-26). Use when editing world scaffold files. GitHub Template sync retired (D-84).
paths: toolchain/template/world/**
---

# World scaffold (mapkeeper)

**Decisions:** D-26 embedded scaffold · **D-84** GitHub Template distribution retired · D-09 wizard primary for writers.

## Source of truth

Edit **`toolchain/template/world/`** only — not legacy `toolchain/cursor/` hand-copy as author UX.

`/user` ships **inside** the scaffold bundle at `.cursor/commands/user.md` and is created with new worlds via the editor.

## Do not

- Reintroduce sync to `mapkeeper-world-template` or `MAPKEEPER_WORLD_TEMPLATE_PAT`.
- Document «Use this GitHub template» as author onboarding.
- Put private maintainer text in the public kit.

## Author layout *(world folder)*

`mapkeeper.toml`, `map/`, `canon/`, `profiles/`, `data/`, `journal/`, `.cursor/commands/user.md`, `AGENTS.md`

## Gate

Scaffold changes are product work — only through approved `/real` scope with D-* backing.
