# Map state

Machine-readable world state on the hex grid. Cell grid: hex, axial `q`/`r`
coordinates, manual paint in the editor. Cell address = `cell_id` — see
[`profiles/README.md`](../profiles/README.md).

This is **map state**, not author description. Human-facing place names/notes
live in `profiles/` (which is *not* a layer). Both are anchored by the same
`cell_id`.

## Layout

- `manifest.json` — bounds + declared layers (index of this world's map state).
- `layers/<id>.json` — one layer per aspect of world state.

## Layers (V0)

- `layers/terrain.json` — `categorical` terrain per cell (legacy proof layer).
- `layers/elevation.json` — sparse integer elevation per cell.
- Hydro (`land`/`water`) is derived from elevation threshold:
  `elevation <= 0` => water, `elevation > 0` => land.

More layers (rivers, regions, routes, settlements) come
later; each is additive and anchored by `cell_id`.

## Partial state (`unknown` / `none` / `value`)

You do not have to fill every cell. Each cell is one of three states per layer:

- **unknown** — not filled / not decided → the cell is simply absent from the file.
- **none** — explicitly absent → `{ "state": "none" }`.
- **value** — a concrete known value → `{ "state": "value", "value": "forest" }`.

The editor writes these; agents can read them directly from the JSON.
