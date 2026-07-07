# MAPKEEPER Code Map (Lite)

Short routing index for agents and maintainers.

Use this first, then open only the needed files.

## Task -> first path

- Core rules, ids, geometry, profile model -> `crates/core/src/`
- Spatial contract (distance/ring/range/bounds) -> `crates/core/src/hex.rs`
- Map size presets (Small/Medium/Large/Epic/Grand/World -> hex-rectangle 16:9 W×H) -> `crates/core/src/map_preset.rs`
- Map state model (dense layers, unknown/none/value, manifest) -> `crates/core/src/layer.rs`
- Cell index (`(q,r) <-> linear index`, `MapBounds::index_of`/`from_index`/`len`) -> `crates/core/src/hex.rs`
- Dense typed-layer model (index-addressed, palette categorical + integer; `read_or_empty`; generic wire `WireCellState`/`LayerCellWrite`) -> `crates/core/src/layer.rs` (`DenseLayer`)
- Elevation/hydro threshold model (`elevation <= 0 => water`) -> `crates/core/src/hydro.rs`
- HTTP API, world file I/O, launcher endpoints -> `crates/server/src/`
- Generic layer endpoints (`GET /api/layers/:id`, `PUT /api/layers/:id/batch`, `PUT /api/layers/:id/cells/:q/:r`) -> `crates/server/src/lib.rs`
- CLI commands and query flow (`profile`, `terrain`, `elevation`, generic `layer <id>`) -> `crates/cli/src/`
- Dense-on-disk layer I/O (`read_or_empty` + `write_dense_layer`; created on first write) -> `crates/core/src/layer.rs`, `crates/server/src/lib.rs`, `crates/cli/src/main.rs`
- Web UI (WASM canvas, Home/Editor flow, tool dock + hydro brush; reads dense elevation, paints via generic batch) -> `crates/web/src/`
- Desktop shell (Tauri wrapper, native dialog bridge) -> `crates/desktop/src/`
- Data contracts and fixtures -> `schemas/`, `fixtures/`
- World scaffold source -> `toolchain/template/world/`
- CI/build behavior -> `.github/workflows/`

## Key files

- Workspace members: `Cargo.toml`
- Core boundary entry: `crates/core/src/lib.rs`
- Server boundary entry: `crates/server/src/lib.rs`
- Web boundary entry: `crates/web/src/lib.rs`
- Home screen layout entry: `crates/web/index.html`
- Editor tool dock (rail + collapsible drawers: Inspect/profile, Hydro brush with size 1x–4x + hover preview + debounced autosave + batch save PUT, View stub, World): `crates/web/index.html`, `crates/web/src/lib.rs` — **overlays** the map (D-39); canvas stable on drawer toggle
- Project list actions (`open` / `remove` / `delete`, with secondary manage flow): `crates/web/src/lib.rs`, `crates/server/src/lib.rs`
- Default create path suggestion (`Documents/MAPKEEPER Worlds`): `crates/server/src/lib.rs`, `crates/web/src/lib.rs`
- Map bounds at create (`map_preset`, `write_map_manifest`, `/api/map` bounds + `legacy_map`): `crates/server/src/lib.rs`, `crates/core/src/map_preset.rs`
- Home Create/Generate preset selectors + Grand/World size warnings + bounds-driven redraw: `crates/web/index.html`, `crates/web/src/lib.rs`
- CLI `init --map-preset`: `crates/cli/src/main.rs`
- Desktop boundary entry: `crates/desktop/src/lib.rs`
- CLI boundary entry: `crates/cli/src/main.rs`

## Boundary rule

- New procedural/generative map logic goes to `crates/core`.
- `web`, `server`, `desktop`, `cli` call into core and own adapter concerns only.

## Model boundary (D-36)

- **Map state = layers** under `map/layers/<id>.json` (machine-readable;
  missing cell = `unknown`). Model in `core::layer`.
- **On-disk shape = dense (`DenseLayer`, `schema_version: 2`)** everywhere
  (scale-layers D-46): index-addressed, palette-encoded categorical + integer.
  The sparse v1 model (`Layer`/`ElevationLayer`) was **removed**; old files are
  not migrated. Layer files are created on first write, sized to the map bounds;
  a `GET` of an absent layer returns an empty typed layer.
- **Hydro projection** derives from integer elevation (`core::hydro`):
  `elevation <= 0` => water, `> 0` => land. Web computes hydro client-side over
  the dense elevation layer.
- **Author profiles** (`profiles/*.json`) are human-facing and **not** a layer.
- **scale-layers (D-46) — adapters slice (done, server + cli + web):** one
  generic layer API by id — `GET /api/layers/:id` returns the dense layer,
  `PUT /api/layers/:id/batch` (`[LayerCellWrite]`) and
  `PUT /api/layers/:id/cells/:q/:r` (`WireCellState`) write cells. Value kind is
  resolved against the layer's `value_type` (categorical string / integer);
  `elevation` defaults to integer, other new ids to categorical. Web reads the
  dense elevation layer and flushes paints via the generic batch. CLI keeps
  `terrain`/`elevation` plus generic `layer get/set/list/clear <id>`. Dense
  schema: `schemas/map-layer-dense.schema.json` (v2); fixtures
  `fixtures/layers-dense/`.
- Renderer is a projection of the layer model, not the source of truth.
- Renderer layout (4.2): fit-to-window canvas + camera viewport — base
  `hex_layout`/`map_half_extent` plus `zoom` (0.6x–2.5x), `pan` (LMB drag),
  and visible draw culling (`visible_scan_bounds`) in `crates/web/src/lib.rs`.
- Future generators/validators are local tools over these layers (not built,
  not AI runtime).
