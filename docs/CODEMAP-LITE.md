# MAPKEEPER Code Map — Lite

Active product-shell routing:

| Need | Start here |
|---|---|
| World identity + spatial config (`mapkeeper.toml`) | `crates/core/src/world.rs` |
| Projects registry shape | `crates/core/src/projects.rs` |
| Spatial foundation + relief field | `crates/core/src/spatial/` |
| Hard-disk brush + Airbrush rate (N-021/N-022) | `crates/core/src/spatial/brush.rs` |
| Extent presets + Create catalog (N-014/N-016) | `crates/core/src/spatial/presets.rs` |
| World ↔ grid ↔ screen conversions | `crates/core/src/spatial/convert.rs` |
| Health and static server | `crates/server/src/lib.rs` |
| Projects create/open/close/delete | `crates/server/src/projects.rs` |
| Create map-preset cards API | `crates/server/src/presets.rs` |
| Spatial persist + stroke + bak restore (N-025) | `crates/server/src/spatial.rs` |
| Atomic file replace + bak | `crates/server/src/atomic_io.rs` |
| Per-world locks + stroke staging | `crates/server/src/state.rs` |
| Projects registry / trash / Create marker | `crates/server/src/world_io.rs` |
| Thin shell document (N-027) | `crates/web/index.html` + `styles.css` + `main.js` |
| Workspace state owner (sole) | `crates/web/workspace-state.js` |
| CRS renderer / camera / relief / stroke client | `crates/web/renderer.js` · `camera.js` · `relief-tool.js` · `spatial-transaction.js` |
| Home/Create/Delete + HTTP | `crates/web/worlds.js` · `api.js` |
| Pure shell math (unit-tested) | `crates/web/shell-math.js` |
| WASM bootstrap + shared pick helpers | `crates/web/src/lib.rs` · `wasm-api.js` |
| Desktop launch | `crates/desktop/src/lib.rs` |
| Local checks | `scripts/check.ps1` |
| Archive isolation guard | `scripts/check_archive_isolation.py` |
| Doc drift (spatial vs identity-only) | `scripts/check_doc_drift.py` |
| Headless spatial smoke | `scripts/smoke-headless.ps1` |
| Relief render scale bench (N-026) | `scripts/bench-render-scale.mjs` + `docs/perf/relief-render-scale-report.json` |
| CRS renderer (cull/cache/rAF) | `crates/web/renderer.js` + `probe_grid_centers` in `crates/web/src/lib.rs` |

Immutable spatial config: `[spatial]` in `mapkeeper.toml` (meters, extent,
`neighbor_center_distance_m`, cols/rows). Mutable content: `spatial/state.json`
(no screen/camera; no ambiguous `cell_size`). Author field id: `relief`.
Geometry stub stays on disk/API for contract tests.
Create: map-size cards from `GET /api/map-presets` → `preset_id` on create
(N-016…N-018, N-020; name + km + area_km2 + cells + Default; no preview
glyph; no cost tier; Default Frontier/`wide_2000`; catalog ≤50k (N-026
measured support evidence in `docs/perf/`); no Editor
resize).
Editor: tool strip under modes (View|Relief); left props; right Details later;
View layers Empty/relief + session Grid outline overlay; Relief Raise/Lower
hard-disk brush (Stamp|Airbrush, radius, Rate, hover, drag) + Edit ocean unlock
(N-002, N-010, N-011, N-013, N-015, N-021, N-022). Hex fills tile flush;
outlines are display-only. Details (right): Relief-only hover cell q,r +
elevation (not full inspector).

The old map renderer, layers, generators, hydrology, profiles, History,
schemas, fixtures, and tests are under `archive/map-v2/`. They are research
material, not active routing targets. See `archive/map-v2/README.md`.
