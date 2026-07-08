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
- Step-3 silhouette model (`land_mask`, six layout classes + 30-recipe pattern bank, shore character, inland sea, elevation sync) -> `crates/core/src/land_mask.rs` (`world-pipeline--land-silhouette-v1`, `step3-geo-variant-classes-v1`, `step3-layout-pattern-bank-v1`)
- Elevation/hydro threshold model (`elevation <= 0 => water`) + stamp falloff math -> `crates/core/src/hydro.rs` (`elevation-authoring-v2`: `filled_elevation_layer`, `stamp_delta`)
- River catalog + `river_id` dense sync (`map/rivers.json`, neighbor chain validation) -> `crates/core/src/rivers.rs` (`river-overlay-layer-v1`, D-54)
- Elevation-driven river auto-generation (flux, depression fill, confluence, `parent`/`basin`) -> `crates/core/src/river_flux.rs` (`rivers-auto-from-elevation-v1`, D-55)
- HTTP API, world file I/O, launcher endpoints -> `crates/server/src/`
- Generic layer endpoints (`GET /api/layers/:id`, `PUT /api/layers/:id/batch`, `PUT /api/layers/:id/cells/:q/:r`) -> `crates/server/src/lib.rs`
- River catalog API (`GET/PUT /api/rivers`, `POST /api/rivers/append`, `POST /api/rivers/:id/pop`, `DELETE /api/rivers/:id`) + `river_id` sync -> `crates/server/src/lib.rs` (`river-overlay-layer-v1`)
- River generate API (`POST /api/rivers/generate` — replace-all from elevation) -> `crates/server/src/lib.rs` (`rivers-auto-from-elevation-v1`, D-55)
- New-world ocean fill (dense elevation all `0` after bounds) -> `crates/server/src/lib.rs` (`write_map_manifest`), `crates/cli/src/main.rs` (`write_initial_ocean_elevation`)
- CLI commands and query flow (`profile`, `terrain`, `elevation`, generic `layer <id>`) -> `crates/cli/src/`
- Dense-on-disk layer I/O (`read_or_empty` + `write_dense_layer`) -> `crates/core/src/layer.rs`, `crates/server/src/lib.rs`, `crates/cli/src/main.rs`
- Web UI (WASM canvas, Home/Editor flow, tool dock + terrain brushes; elevation view palette/labels/peaks in `elevation_view.rs`) -> `crates/web/src/`
- Rivers tool dock (chain-click brush, stroke overlay, erase whole river, Generate rivers + confirm) -> `crates/web/index.html`, `crates/web/src/lib.rs` (`river-overlay-layer-v1`, `rivers-auto-from-elevation-v1`)
- Perf Step 0 measurement hooks (`open_ms`, layer fetch/parse/mirror, `redraw_ms`, `batch_flush_ms`; `#view-perf` + console) -> `crates/web/src/lib.rs` (`perf-100k--measurement-hooks`)
- Web dense elevation client (index-addressed `DenseLayer` render cache, no HashMap mirror) -> `crates/web/src/lib.rs` (`perf-100k--web-dense-client`)
- rAF redraw coalescing (`schedule_redraw`, one draw per animation frame) -> `crates/web/src/lib.rs` (`perf-100k--raf-redraw-coalesce`)
- Canvas LOD: adaptive grid stroke + profile marker zoom cutoff -> `crates/web/src/lib.rs` (`perf-100k--canvas-lod-grid-markers`, **D-51** grid lines seamless: `FILL_SCALE_GRID_ON/OFF`, rename toggle)
- Desktop shell (Tauri wrapper, native dialog bridge) -> `crates/desktop/src/`
- Desktop launch defaults (maximized on startup) -> `crates/desktop/src/lib.rs` (`desktop-maximized-default-launch`)
- Data contracts and fixtures -> `schemas/`, `fixtures/`
- River dogfood fixture worlds (Small elevation presets) -> `fixtures/worlds/` (`river-dogfood-fixture-worlds`; maintainer/CI — no Home UI per D-59)
- Build wizard draft state (`[build]` in `mapkeeper.toml`, read/write/clear) -> `crates/core/src/build_state.rs` (`home-build-draft-v1`, D-59)
- Build draft API (`POST /api/projects` `build_wizard`, `PUT /api/build`, list `build_draft`/`build_step`) -> `crates/server/src/lib.rs` (D-59)
- Build wizard step-3 API (`POST /api/build/land-mask/generate`, `PUT /api/build/land-mask/cells`) -> `crates/server/src/lib.rs` (`world-pipeline--land-silhouette-v1`)
- World Build Wizard shell (D-57 + D-59 draft resume): Home **Build World**, fullscreen overlay, Save Draft / wizard resume -> `crates/web/index.html`, `crates/web/src/lib.rs`
- World Build Wizard step 3 controls (ordered blocks 1..4; A/B/C = distinct layout classes + recipes; Regenerate reshuffles trio; shore orthogonal; Continue only) -> `crates/web/index.html`, `crates/web/src/lib.rs` (`step3-land-silhouette-flow-v2`, `step3-geo-variant-classes-v1`, `step3-layout-pattern-bank-v1`)
- World scaffold source -> `toolchain/template/world/`
- CI/build behavior -> `.github/workflows/`

## Key files

- Workspace members: `Cargo.toml`
- Full symbol codemap (generated): `docs/CODEMAP.md`
- Codemap generator script: `scripts/gen_codemap.py`
- Core boundary entry: `crates/core/src/lib.rs`
- Server boundary entry: `crates/server/src/lib.rs`
- Web boundary entry: `crates/web/src/lib.rs`
- Home screen layout entry: `crates/web/index.html`
- World Build Wizard overlay (D-57 shell + D-59 draft): `crates/web/index.html` (`#build-wizard`), `crates/web/src/lib.rs` (`open_build_wizard`, `persist_build_draft`, `wizard_return_home`)
- Editor tool dock (rail + collapsible drawers: Inspect/profile, Terrain brushes Land/Water/Raise/Lower + step/falloff, View color mode + elevation overlays, World): `crates/web/index.html`, `crates/web/src/lib.rs`, `crates/web/src/elevation_view.rs` — **overlays** the map (D-39); canvas stable on drawer toggle
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
