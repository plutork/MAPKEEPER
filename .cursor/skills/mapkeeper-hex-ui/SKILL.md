---
name: mapkeeper-hex-ui
description: >-
  Local web UI for mapkeeper hex map editor — writer/GM UX, intentional design,
  hex canvas patterns. Use when building or restyling src/ editor UI, map
  renderer, or wizard screens.
paths: crates/web/**
---

# Hex map UI (mapkeeper V0)

**Adapted from:** [anthropics/skills frontend-design](https://github.com/anthropics/skills/tree/main/skills/frontend-design) — trimmed for mapkeeper.

**Companion:** MAPKEEPER-OS `mapkeeper-product.mdc` · D-12 web-first V0.

## Audience

**Writer/GM** — not a developer. Controls use plain words: «Save map», not «Persist grid state».

## Product job (V0)

1. Minimal wizard → world folder + scaffold.
2. **Hex blank map** — manual paint cells.
3. Per-cell profile edit → JSON under `profiles/`.
4. No canon editor, no import, no git required.

## Stack (D-20)

`crates/web` — Rust → WASM (Leptos/Yew/Dioxus TBD); calls `mapkeeper-core`
for hex/validation logic, calls `mapkeeper-server`/Tauri-adapter for
filesystem. No direct FS access from this crate.

## Design discipline *(from frontend-design, compressed)*

- **One signature** — e.g. hex grid as hero; keep chrome quiet.
- **Typography:** deliberate pair — not default Inter/Roboto stack without reason.
- **Motion:** optional; respect `prefers-reduced-motion`.
- **Copy:** sentence case, active voice, errors say how to fix.
- Avoid AI-slop palettes (cream+terracotta, neon-on-black) unless brief asks.

## Hex renderer hints *(roadmap 4.2 — Shape may lock stack)*

- Prefer **canvas or SVG** for axial hex grid; hit-test by cell q,r.
- Show `display_name` on hover; edit opens profile panel.
- Performance: only redraw dirty cells on large maps (later).

## Technical

- Local web first (D-12); no Tauri in V0.
- Do not embed AI runtime — agents use CLI/query outside editor.

## Gate

Implementation only via MAPKEEPER-OS **`/real`** after layout + renderer Shape (`/idea`).
