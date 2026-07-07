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

- `layers/terrain.json` — `categorical` terrain per cell.
- `layers/elevation.json` — `integer` elevation per cell.
- Hydro (`land`/`water`) is derived from elevation threshold:
  `elevation <= 0` => water, `elevation > 0` => land.

**New worlds start as ocean:** create/init writes a dense `elevation` layer
with every cell at `0` (sea level), sized to the map bounds. You paint islands
with Raise/Lower or Land presets in the editor.

Older worlds without an elevation file still resolve unknown cells as land
(`DEFAULT_LAND_ELEVATION=1`) until painted or recreated.

More layers (rivers, regions, routes, settlements) come later; each is additive
and anchored by `cell_id`.

## On-disk shape (dense) + partial state

Layers are **dense**: cells are addressed by their linear index within the map
bounds (not by `cell_id` string), and categorical values are
palette/dictionary-encoded — this keeps 50–100k-cell maps compact
(`schemas/map-layer-dense.schema.json`). `cell_id` stays the identity you use
from the API and agents.

You do not have to fill every cell. Each cell is one of three states per layer:

- **unknown** — not filled / not decided.
- **none** — explicitly absent.
- **value** — a concrete known value (e.g. `"forest"`, or an integer).

The editor writes these; agents read them via the API (`GET /api/layers/<id>`).
