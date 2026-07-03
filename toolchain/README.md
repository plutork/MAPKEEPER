# Author toolchain

Templates authors copy into **world workspaces** — a separate repo or folder for lore, maps, and canon. Do **not** install into the mapkeeper product repo root.

## Layout

| Shipped here | Install to (world repo) | Purpose |
|--------------|-------------------------|---------|
| [cursor/user.md](cursor/user.md) | `.cursor/commands/user.md` | `/user` — author stance in Cursor |

## Quick start

1. Open or create your **world** repo in Cursor.
2. Follow [cursor/README.md](cursor/README.md) to install `/user`.
3. Use mapkeeper data contracts and UI from your world workspace — not editor source.

## Later (V0+)

Planned additions in this tree (not shipped yet):

- `schemas/` — profile and validation templates
- `cursor/user-*.md` — specialist lenses (Geo, Time, Import, Canon)
- init helper — copy script or CLI (`mapkeeper init`), TBD

Maintainer tooling lives outside this public repo.
