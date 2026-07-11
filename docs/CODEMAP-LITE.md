# MAPKEEPER Code Map (Lite)

Short routing index for agents and maintainers.

Use this first, then open only the needed files.

Product pitch / invariants (authors): `README.md#product` · `README.md#invariants` (`STARTER_PACK.md` is a redirect stub, D-74).

## Task -> first path

- Core rules, ids, geometry, profile model -> `crates/core/src/`
- **World generation pipeline (D-92)** — dependency order land → coast/plates → geology → elevation → climate → hydrology -> `crates/core/src/worldgen/`
- Spatial contract (distance/ring/range/bounds) -> `crates/core/src/hex.rs`
- Map size presets (Small/Medium/Large/Epic/Grand/World -> hex-rectangle 16:9 W×H; D-73 ladder Small~510…World~100k) -> `crates/core/src/map_preset.rs` (`map-preset--ladder-retune-500`)
- Map state model (dense layers, unknown/none/value, manifest) -> `crates/core/src/layer.rs`
- Cell index (`(q,r) <-> linear index`, `MapBounds::index_of`/`from_index`/`len`) -> `crates/core/src/hex.rs`
- Dense typed-layer model (index-addressed, palette categorical + integer; `read_or_empty`; generic wire `WireCellState`/`LayerCellWrite`) -> `crates/core/src/layer.rs` (`DenseLayer`)
- Step-3 silhouette model (`land_mask`, six layout classes + growth-plan catalog, seeded layered land growth → cleanup, shore character, inland sea, elevation sync; UI: 6 class cards + recipe-only Regenerate below cards + always-visible gen identity line; Continents balance + Archipelago multi-island enforce, incl. `archipelago_twin_groups` anti-blob retune) -> `crates/core/src/worldgen/land.rs` (`world-pipeline--land-silhouette-v1`, D-62…D-66 / D-68 / `step3-organic-silhouette-v1`; legacy import `mapkeeper_core::land_mask`)
- Step-4 geology model (`geology` categorical, **D-87 hidden plate substrate** in `worldgen/plates.rs`, boundary-distance field, probabilistic orogenic width/gaps along plate edges, styles belts/shields/arcs/random) -> `crates/core/src/worldgen/geology/` (`hidden-plates-geology-foundation`, D-63, D-87; legacy `mapkeeper_core::geology`)
- Step-5 elevation bridge (`worldgen/elevation/`: geology band jitter + class-aware hex smooth; **D-89** Standard/Bold/Chaos; D-88 amends D-72; seed from world_id + style + regenerate_nonce) -> `crates/core/src/worldgen/elevation/`, `crates/server/src/build.rs` (`POST /api/build/elevation/generate`), `crates/web/src/wizard.rs`, `crates/web/index.html` (legacy `mapkeeper_core::elevation_gen`)
- Step-6 coast foundation (auto `coast_distance` from `land_mask`; no wizard UI; **D-90**) -> `crates/core/src/worldgen/coast.rs` (legacy `mapkeeper_core::coast_distance`)
- Climate T2 zonal heuristic (`temperature`/`precipitation`/`ice`; internal west wind; wizard step 5; **D-90**) -> `crates/core/src/worldgen/climate/`, `crates/server/src/build.rs` (`POST /api/build/climate/generate`), `crates/web/src/wizard.rs` (legacy `mapkeeper_core::climate`)
- Elevation/hydro threshold model (`elevation <= 0 => water`) + stamp falloff math -> `crates/core/src/hydro.rs` (`elevation-authoring-v2`: `filled_elevation_layer`, `stamp_delta`)
- River catalog + `river_id` dense sync (`map/rivers.json`, neighbor chain validation) -> `crates/core/src/rivers.rs` (`river-overlay-layer-v1`, D-54)
- Elevation-driven river auto-generation (flux, confluence, `parent`/`basin`; **D-91** reads `precipitation` when present, uniform fallback; **lake-aware routing** + `RiverDensity` Few/Balanced/Many; **D-100** post-flux trace/plateau, strict mouth invariant, lake outlet streams, `GenerateRiversOutput.rejected_rivers`) -> `crates/core/src/worldgen/hydrology/river_flux.rs`, `river_validate.rs`, `types.rs` (`rivers-auto-from-elevation-v1`, D-55; `rivers-flux-v2--climate-precip`, D-91; `hydrology-river-lake-integration-v1`; `rivers-mouth-tracing-v2`, **D-100**; legacy `mapkeeper_core::river_flux`)
- **Hydrology depression analysis (H0)** — `DepressionAnalysis`: conditioned DEM, `fill_depth`, geometric basin/spill maps; shared by river flux -> `crates/core/src/worldgen/hydrology/depression_fill.rs`, `types.rs` (`hydrology-depression-analysis-v1`, **D-99**; no lakes/UI)
- Lake catalog + `lake_id` dense sync (`map/lakes.json`, atomic persist with layer) -> `crates/core/src/lakes.rs`, `crates/server/src/world_io.rs` (`persist_lakes`), `crates/server/src/lakes.rs` (`GET/PUT /api/lakes`; hydrology-lake-domain-v1)
- Lake generation from depression analysis + precip (`LakeDensity`, catchment supply) -> `crates/core/src/worldgen/hydrology/lakes.rs`; `POST /api/lakes/generate` clears rivers (`hydrology-lake-generation-v1`)
- HTTP API, world file I/O, launcher endpoints -> `crates/server/src/` (**D-96** complete: S0 `state`/`world_io`; S1 `projects.rs`; S2 `build.rs`; S3 `layers.rs`; S4 `rivers.rs`; `lib.rs` facade)
- Map/profile/layer endpoints (`GET /api/map`, profile `GET/PUT`, generic `/api/layers/*`) -> `crates/server/src/layers.rs` (D-96 S3)
- River catalog API (`GET/PUT /api/rivers`, `POST /api/rivers/append`, `POST /api/rivers/:id/pop`, `DELETE /api/rivers/:id`) + `river_id` sync -> `crates/server/src/rivers.rs` (`river-overlay-layer-v1`, D-96 S4)
- River generate API (`POST /api/rivers/generate` — replace-all; optional `{ river_density, regenerate_nonce }`; reads lake catalog when present; `precip_source` + `river_density` + `rejected_river_count` in response; persist `river_id` from catalog not raw owners) -> `crates/server/src/rivers.rs`, `world_io.rs` (`rivers-auto-from-elevation-v1`, D-55; D-91 climate precip; `hydrology-river-lake-integration-v1`; **D-100**)
- New-world ocean fill (dense elevation all `0` after bounds) -> `crates/server/src/world_io.rs` (`rewrite_world_bounds`), `crates/server/src/projects.rs` (create → `write_map_manifest`), `crates/cli/src/main.rs` (`write_initial_ocean_elevation`)
- CLI commands and query flow (`profile`, `terrain`, `elevation`, generic `layer <id>`) -> `crates/cli/src/`
- Dense-on-disk layer I/O (`read_or_empty` + `write_dense_layer`) -> `crates/core/src/layer.rs`, `crates/server/src/world_io.rs`, `crates/cli/src/main.rs`
- Web UI (WASM canvas, Home/Editor flow, tool dock + terrain brushes; elevation view palette/labels/peaks in `elevation_view.rs`) -> `crates/web/src/` (**D-94** split: `state.rs`, `dom.rs`, `api.rs`, `canvas.rs`, `brush.rs`, `wizard.rs`, `editor.rs`, `home.rs`; `lib.rs` = `start()` facade)
- Rivers tool dock (chain-click brush, stroke overlay, erase whole river, Generate rivers + confirm) -> `crates/web/index.html`, `crates/web/src/editor.rs` (`river-overlay-layer-v1`, `rivers-auto-from-elevation-v1`)
- Wizard/editor water generation UI (lake + river density presets, generate lakes/rivers, invalidation copy; lake overlay render) -> `crates/web/index.html`, `crates/web/src/wizard.rs`, `crates/web/src/editor.rs`, `crates/web/src/api.rs`, `crates/web/src/canvas.rs` (`hydrology-water-generation-ui-v1`; closes Phase 1)
- Water gen dogfood diagnostics (snapshot + last action trace; wizard + editor) -> `crates/web/src/water_diag.rs`, `crates/web/src/api.rs` (`hydrology-dogfood-gen-diagnostics`)
- Perf Step 0 measurement hooks (`open_ms`, layer fetch/parse/mirror, `redraw_ms`, `batch_flush_ms`; `#view-perf` + console) -> `crates/web/src/lib.rs` + `canvas.rs` (`perf-100k--measurement-hooks`)
- Web dense elevation client (index-addressed `DenseLayer` render cache, no HashMap mirror) -> `crates/web/src/lib.rs` + `canvas.rs` (`perf-100k--web-dense-client`)
- rAF redraw coalescing (`schedule_redraw`, one draw per animation frame) -> `crates/web/src/canvas.rs` (`perf-100k--raf-redraw-coalesce`)
- Canvas LOD: adaptive grid stroke + profile marker zoom cutoff -> `crates/web/src/canvas.rs` (`perf-100k--canvas-lod-grid-markers`, **D-51** grid lines seamless: `FILL_SCALE_GRID_ON/OFF`, rename toggle)
- Desktop shell (Tauri wrapper, native dialog + opener bridges) -> `crates/desktop/src/`, `crates/desktop/capabilities/default.json`
- Desktop launch defaults (maximized on startup) -> `crates/desktop/src/lib.rs` (`desktop-maximized-default-launch`)
- Data contracts and fixtures -> `schemas/`, `fixtures/`
- River dogfood fixture worlds (fixed 14×8 elevation presets; not author Small) -> `fixtures/worlds/` (`river-dogfood-fixture-worlds`; maintainer/CI — no Home UI per D-59)
- Build wizard draft state (`[build]` in `mapkeeper.toml`, steps 1–4 + `scheme`, read/write/clear/normalize) -> `crates/core/src/build_state.rs` (`home-build-draft-v1`, D-59, D-69, D-71)
- Build draft API (`POST /api/projects` `build_wizard`, `PUT /api/build`, `PUT /api/build/bounds`, list `build_draft`/`build_step`) -> `crates/server/src/projects.rs`, `crates/server/src/build.rs` (D-59, D-69, D-71)
- Build wizard step-1–4 API (`PUT /api/build/bounds` preset rewrite + Geo reset; `POST /api/build/land-mask/generate` returns seed identity JSON; land-mask cells; geology/elevation/climate generate) -> `crates/server/src/build.rs` (`wizard-merge-size-grid`, `world-pipeline--land-silhouette-v1`, `world-pipeline--tectonics-v1`, D-68/D-69/D-71)
- World Build Wizard shell (D-57 + D-59 draft resume + D-71 size+grid one screen): Home **Build World**, fullscreen overlay, Save Draft / wizard resume + Home footer version label -> `crates/web/index.html`, `crates/web/src/wizard.rs`, `crates/web/src/home.rs`
- Home version label only (D-80 supersedes D-76 Check-for-updates CTA for alpha; updates via `update.ps1` / daily `run.ps1` pull when clean) -> `crates/web/index.html`, `crates/web/src/lib.rs`
- Tester first-run flow (D-77): empty Home primary CTA `Create your first world` -> Build wizard defaults; blank Create demoted to advanced; post-Finish next-step note -> `crates/web/index.html`, `crates/web/src/home.rs`
- Agent-managed alpha (D-80…D-86): root `setup.ps1` / daily `run.ps1` (pull when clean) / optional `update.ps1` + Cursor `/doctor`
- World Build Wizard steps 1–4 (size+blank grid → silhouette → tectonics → elevation → Finish; Back on steps 2–4 via in-app confirm; step 2 gen identity + Edit brush S–XL zoom-adaptive with pan blocked during edit drag and in-flight stamp queue guard for larger brush tiers; step 3 geology contrast+legend) -> `crates/web/index.html`, `crates/web/src/wizard.rs` (`wizard-merge-size-grid`, `brush-size--zoom-adaptive`, `geology-readable--preview-contrast`, D-43/D-70/D-65/D-66/D-68/D-69/D-71/D-72, `world-pipeline--tectonics-v1`)
- Brush size S–XL screen tiers → effective hex radius from zoom (editor + wizard Edit; cap 24) -> `crates/web/src/brush.rs` (`brush-size--zoom-adaptive`, D-70)
- Wizard confirm overlay (`#wiz-confirm-overlay`) for Back / bounds reset — avoids silent `window.confirm` in Tauri -> `crates/web/index.html`, `crates/web/src/wizard.rs` (D-69)
- World scaffold source -> `toolchain/template/world/`
- CI/build behavior -> `.github/workflows/` (`ci.yml`; NSIS alpha release workflow removed under D-80)

## Key files

- Workspace members: `Cargo.toml`
- Developer setup/runbook: `docs/DEV.md`
- Cursor alpha guide (D-80/D-81): `docs/CURSOR-ALPHA.md`
- Full symbol codemap (generated): `docs/CODEMAP.md`
- Codemap generator script: `scripts/gen_codemap.py`
- Codemap drift CI guard: `scripts/check_codemap_drift.py` (regen `CODEMAP.md` + validate `CODEMAP-LITE.md` paths)
- Alpha Windows bootstrap/launch/update: `setup.ps1`, `run.ps1` (D-86 pull-in-run), `update.ps1`; troubleshooting: `.cursor/commands/doctor.md`
- Core boundary entry: `crates/core/src/lib.rs` (facade; worldgen under `worldgen/`, legacy top-level re-exports for adapters)
- Server boundary entry: `crates/server/src/lib.rs` (facade: `ServerConfig`, `build_router`, `bind`, `run` — **D-96** complete)
- Server launcher/projects API: `crates/server/src/projects.rs` (`/api/projects*`, `/api/fixture-worlds*` — D-96 S1)
- Server build wizard API: `crates/server/src/build.rs` (`/api/build*`, pipeline generate — D-96 S2)
- Server map/profile/layers API: `crates/server/src/layers.rs` (`/api/map`, profile, `/api/layers/*` — D-96 S3)
- Server rivers API: `crates/server/src/rivers.rs` (`/api/rivers/*`, generate — D-96 S4)
- Server lakes API: `crates/server/src/lakes.rs` (`GET/PUT /api/lakes` — hydrology-lake-domain-v1)
- Server shared state: `crates/server/src/state.rs` (`AppState`, `ActiveWorld` — D-96 S0)
- Server world/layer I/O helpers: `crates/server/src/world_io.rs` (manifest, bounds, projects path, dense layer read/write — D-96 S0)
- Web boundary entry: `crates/web/src/lib.rs` (`start()` + wiring only; D-94 complete)
- Web state/types/DTOs: `crates/web/src/state.rs` (D-94 B1)
- Web DOM helpers: `crates/web/src/dom.rs` (D-94 B1)
- Web HTTP fetch/load/post: `crates/web/src/api.rs` (D-94 B1)
- Web canvas layout/redraw/rAF: `crates/web/src/canvas.rs` (D-94 B2)
- Web brush tiers + paint stamps: `crates/web/src/brush.rs` (D-94 B4)
- Web Build Wizard UI/handlers: `crates/web/src/wizard.rs` (D-94 B3)
- Web editor canvas/dock handlers: `crates/web/src/editor.rs` (D-94 B4)
- Web Home/project list/create: `crates/web/src/home.rs` (D-94 B4)
- Web elevation view overlays: `crates/web/src/elevation_view.rs`
- Home screen layout entry: `crates/web/index.html`
- Alpha tester notes (stub → CURSOR-ALPHA): `docs/TESTER-NOTES-0.2.1.md`
- World Build Wizard overlay (D-57 shell + D-59 draft): `crates/web/index.html` (`#build-wizard`), `crates/web/src/wizard.rs`
- Editor tool dock (rail + collapsible drawers: Inspect/profile, Terrain brushes Land/Water/Raise/Lower + step/falloff, View color mode + elevation overlays, World): `crates/web/index.html`, `crates/web/src/editor.rs`, `crates/web/src/elevation_view.rs` — **overlays** the map (D-39); canvas stable on drawer toggle
- Project list actions (`open` / `remove` / `delete`, with secondary manage flow): `crates/web/src/home.rs`, `crates/server/src/projects.rs`
- Default create path suggestion (`Documents/MAPKEEPER Worlds`): `crates/server/src/projects.rs`, `crates/web/src/home.rs`
- Map bounds at create (`map_preset`, `write_map_manifest`, `/api/map` bounds + `legacy_map`): `crates/server/src/projects.rs`, `crates/server/src/world_io.rs`, `crates/server/src/layers.rs` (`GET /api/map`), `crates/core/src/map_preset.rs`
- Home Create/Generate preset selectors + Grand/World size warnings + bounds-driven redraw: `crates/web/index.html`, `crates/web/src/home.rs`
- CLI `init --map-preset`: `crates/cli/src/main.rs`
- Desktop boundary entry: `crates/desktop/src/lib.rs`
- CLI boundary entry: `crates/cli/src/main.rs`

## Boundary rule

- New procedural/generative map logic goes to `crates/core`, preferably under `crates/core/src/worldgen/` by pipeline stage (D-92).
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
  dense elevation layer and flushes paints via the generic batch. Wizard land
  Edit (D-70): optimistic local stamps on every new center cell during drag;
  HTTP flush on mouseup only; brush-size click must not redraw huge hex
  previews (circle preview when radius > 2; clear hover over wiz-right);
  effective radius = max(zoom-derived, tier) so S/M/L/XL stay distinct up close.
  Server cell PUT patches touched cells. CLI keeps
  `terrain`/`elevation` plus generic `layer get/set/list/clear <id>`. Dense
  schema: `schemas/map-layer-dense.schema.json` (v2); fixtures
  `fixtures/layers-dense/`.
- Renderer is a projection of the layer model, not the source of truth.
- Renderer layout (4.2): fit-to-window canvas + camera viewport — base
  `hex_layout`/`map_half_extent` plus `zoom` (min 0.6; max from target
  on-screen hex ≈40px — `zoom-cap--target-hex-px` D-85, amends D-41 flat 2.5x),
  `pan` (LMB drag),
  and visible draw culling (`visible_scan_bounds`) in `crates/web/src/canvas.rs`.
- Future generators/validators are local tools over these layers (not built,
  not AI runtime).
