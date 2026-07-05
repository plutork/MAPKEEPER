# World workspace

You are in an **author world repo**, not mapkeeper editor source.

- Use **`/user`** (`.cursor/commands/user.md`) for author stance.
- Read world data under `map/`, `canon/`, `profiles/`, `data/`.
- Do not patch the [mapkeeper](https://github.com/plutork/MAPKEEPER) product repo from here.

## Data contract (V0)

Cell profiles in `profiles/*.json` — one file per cell, fields `cell_id`/`display_name`/`slug`/`notes`. Full description: [`profiles/README.md`](profiles/README.md). The running editor validates on save; no separate schema file lives in this folder.

Product pitch: [STARTER_PACK.md](https://github.com/plutork/MAPKEEPER/blob/main/STARTER_PACK.md)
