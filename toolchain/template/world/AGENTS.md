# World workspace

You are in an **author world repo**, not mapkeeper editor source.

- Use **`/user`** (`.cursor/commands/user.md`) for author stance.
- Read world data under `map/`, `canon/`, `profiles/`, `data/`.
- Do not patch the [mapkeeper](https://github.com/plutork/MAPKEEPER) product repo from here.

## Data contract (V0)

Two anchored-by-`cell_id` stores, kept separate on purpose:

- **Author profiles** — `profiles/*.json`, one file per cell, fields `cell_id`/`display_name`/`slug`/`notes` (human-facing description). Full description: [`profiles/README.md`](profiles/README.md).
- **Map state** — `map/manifest.json` + `map/layers/<id>.json` (machine-readable world state; `profiles` is **not** a layer). Layers are **dense** (index-addressed, palette-encoded) with the `unknown/none/value` partial-state trio. Build step 3 uses `land_mask` (`ocean`/`land`/`inland_sea`) as silhouette source of truth; elevation stays `integer` (`elevation <= 0` => water) and is synced from accepted `land_mask`. Full description: [`map/README.md`](map/README.md).

The running editor validates on save; no separate schema file lives in this folder.

Product pitch: [STARTER_PACK.md](https://github.com/plutork/MAPKEEPER/blob/main/STARTER_PACK.md)
