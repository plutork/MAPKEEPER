# MAPKEEPER Code Map (Lite)

Short routing index for agents and maintainers.

Use this first, then open only the needed files.

Product pitch / invariants (authors): `README.md#product` · `README.md#invariants` (`STARTER_PACK.md` is a redirect stub, D-74).

## Task -> first path

- Core rules, ids, geometry, profile model -> `crates/core/src/`
- Spatial contract (distance/ring/range/bounds) -> `crates/core/src/hex.rs`
- Map size presets (Small/Medium/Large/Epic/Grand/World -> hex-rectangle 16:9 W×H; D-73 ladder Small~510…World~100k) -> `crates/core/src/map_preset.rs` (`map-preset--ladder-retune-500`)
- Map state model (dense layers, unknown/none/value, manifest) -> `crates/core/src/layer.rs`
- Cell index (`(q,r) <-> linear index`, `MapBounds::index_of`/`from_index`/`len`) -> `crates/core/src/hex.rs`
- Dense typed-layer model (index-addressed, palette categorical + integer; `read_or_empty`; generic wire `WireCellState`/`LayerCellWrite`) -> `crates/core/src/layer.rs` (`DenseLayer`)
- Step-3 silhouette model (`land_mask`, six layout classes + growth-plan catalog, seeded layered land growth → cleanup, shore character, inland sea, elevation sync; UI: 6 class cards + recipe-only Regenerate below cards + always-visible gen identity line; Continents balance + Archipelago multi-island enforce, incl. `archipelago_twin_groups` anti-blob retune) -> `crates/core/src/land_mask.rs` (`world-pipeline--land-silhouette-v1`, D-62…D-66 / D-68 / `step3-organic-silhouette-v1`)
- Step-4 geology model (`geology` categorical, **D-87 hidden plate substrate** in `plates.rs`, boundary-distance field, probabilistic orogenic width/gaps along plate edges, styles belts/shields/arcs/random) -> `crates/core/src/geology.rs`, `crates/core/src/plates.rs` (`hidden-plates-geology-foundation`, D-63, D-87)
- Step-5 elevation bridge (`elevation_gen.rs`: geology band jitter + class-aware hex smooth; **D-89** Standard/Bold/Chaos; D-88 amends D-72; seed from world_id + style + regenerate_nonce) -> `crates/core/src/elevation_gen.rs`, `crates/server/src/lib.rs`, `crates/web/src/lib.rs`, `crates/web/index.html`
- Step-6 coast foundation (auto `coast_distance` from `land_mask`; no wizard UI; **D-90**) -> `crates/core/src/coast_distance.rs`
- Climate T2 zonal heuristic (`temperature`/`precipitation`/`ice`; internal west wind; wizard step 5; **D-90**) -> `crates/core/src/climate.rs`, `crates/server/src/lib.rs`, `crates/web/`
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
- Desktop shell (Tauri wrapper, native dialog + opener bridges) -> `crates/desktop/src/`, `crates/desktop/capabilities/default.json`
- Desktop launch defaults (maximized on startup) -> `crates/desktop/src/lib.rs` (`desktop-maximized-default-launch`)
- Data contracts and fixtures -> `schemas/`, `fixtures/`
- River dogfood fixture worlds (fixed 14×8 elevation presets; not author Small) -> `fixtures/worlds/` (`river-dogfood-fixture-worlds`; maintainer/CI — no Home UI per D-59)
- Build wizard draft state (`[build]` in `mapkeeper.toml`, steps 1–4 + `scheme`, read/write/clear/normalize) -> `crates/core/src/build_state.rs` (`home-build-draft-v1`, D-59, D-69, D-71)
- Build draft API (`POST /api/projects` `build_wizard`, `PUT /api/build`, `PUT /api/build/bounds`, list `build_draft`/`build_step`) -> `crates/server/src/lib.rs` (D-59, D-69, D-71)
- Build wizard step-1–4 API (`PUT /api/build/bounds` preset rewrite + Geo reset; `POST /api/build/land-mask/generate` returns seed identity JSON; land-mask cells; geology/elevation generate) -> `crates/server/src/lib.rs` (`wizard-merge-size-grid`, `world-pipeline--land-silhouette-v1`, `world-pipeline--tectonics-v1`, D-68/D-69/D-71)
- World Build Wizard shell (D-57 + D-59 draft resume + D-71 size+grid one screen): Home **Build World**, fullscreen overlay, Save Draft / wizard resume + Home footer version label -> `crates/web/index.html`, `crates/web/src/lib.rs`
- Home version label only (D-80 supersedes D-76 Check-for-updates CTA for alpha; updates via `/mk-update`) -> `crates/web/index.html`, `crates/web/src/lib.rs`
- Tester first-run flow (D-77): empty Home primary CTA `Create your first world` -> Build wizard defaults; blank Create demoted to advanced; post-Finish next-step note -> `crates/web/index.html`, `crates/web/src/lib.rs`
- Agent-managed alpha (D-80…D-86): root `setup.ps1` / daily `run.ps1` (pull when clean) / optional `update.ps1` + Cursor `/doctor`
- World Build Wizard steps 1–4 (size+blank grid → silhouette → tectonics → elevation → Finish; Back on steps 2–4 via in-app confirm; step 2 gen identity + Edit brush S–XL zoom-adaptive with pan blocked during edit drag and in-flight stamp queue guard for larger brush tiers; step 3 geology contrast+legend) -> `crates/web/index.html`, `crates/web/src/lib.rs` (`wizard-merge-size-grid`, `brush-size--zoom-adaptive`, `geology-readable--preview-contrast`, D-43/D-70/D-65/D-66/D-68/D-69/D-71/D-72, `world-pipeline--tectonics-v1`)
- Brush size S–XL screen tiers → effective hex radius from zoom (editor + wizard Edit; cap 24) -> `crates/web/src/lib.rs` (`brush-size--zoom-adaptive`, D-70)
- Wizard confirm overlay (`#wiz-confirm-overlay`) for Back / bounds reset — avoids silent `window.confirm` in Tauri -> `crates/web/index.html`, `crates/web/src/lib.rs` (D-69)
- World scaffold source -> `toolchain/template/world/`
- CI/build behavior -> `.github/workflows/` (`ci.yml`; NSIS alpha release workflow removed under D-80)

## Key files

- Workspace members: `Cargo.toml`
- Developer setup/runbook: `docs/DEV.md`
- Cursor alpha guide (D-80/D-81): `docs/CURSOR-ALPHA.md`
- Full symbol codemap (generated): `docs/CODEMAP.md`
- Codemap generator script: `scripts/gen_codemap.py`
- Alpha Windows bootstrap/launch/update: `setup.ps1`, `run.ps1` (D-86 pull-in-run), `update.ps1`; troubleshooting: `.cursor/commands/doctor.md`
- Core boundary entry: `crates/core/src/lib.rs`
- Server boundary entry: `crates/server/src/lib.rs`
- Web boundary entry: `crates/web/src/lib.rs`
- Home screen layout entry: `crates/web/index.html`
- Alpha tester notes (stub → CURSOR-ALPHA): `docs/TESTER-NOTES-0.2.1.md`
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
  and visible draw culling (`visible_scan_bounds`) in `crates/web/src/lib.rs`.
- Future generators/validators are local tools over these layers (not built,
  not AI runtime).
