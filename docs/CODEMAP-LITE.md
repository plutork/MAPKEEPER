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
| Projects create/open/close | `crates/server/src/projects.rs` |
| Create map-preset cards API | `crates/server/src/presets.rs` |
| Spatial persist + API | `crates/server/src/spatial.rs` |
| Projects filesystem adapter | `crates/server/src/world_io.rs` |
| Five-mode shell + tool strip (View/Relief) + elevation canvas | `crates/web/index.html` |
| WASM bootstrap + shared pick helpers | `crates/web/src/lib.rs` |
| Desktop launch | `crates/desktop/src/lib.rs` |
| Local checks | `scripts/check.ps1` |
| Archive isolation guard | `scripts/check_archive_isolation.py` |

Immutable spatial config: `[spatial]` in `mapkeeper.toml` (meters, extent,
`neighbor_center_distance_m`, cols/rows). Mutable content: `spatial/state.json`
(no screen/camera; no ambiguous `cell_size`). Author field id: `relief`.
Geometry stub stays on disk/API for contract tests.
Create: map-size cards from `GET /api/map-presets` → `preset_id` on create
(N-016…N-018, N-020; name + km + area_km2 + cells + Default; no preview
glyph; no cost tier; Default Frontier/`wide_2000`; catalog ≤50k; no Editor
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
