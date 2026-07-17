# map-v2 archive

Read-only reference snapshot of the map implementation that was active at
commit `53d286f2a2673a6e18ea093dd562f62db568c90a`.

This directory is not an active product surface. It is excluded from the Cargo
workspace, CI, tests, linting, coverage, runtime, type generation, and release
bundles. Active code must not import, include, or path-depend on it.

Do not fix, extend, refactor, or feature-gate this archive. Study it, capture a
useful conclusion in the future architecture documentation, then advance the
corresponding entry in [RETIREMENT.md](RETIREMENT.md).

## Structure

| Area | Archived location | State at archival |
|---|---|---|
| Map rendering | `crates/web/src/canvas.rs`, `elevation_view.rs` | Working Canvas 2D hex projection; tightly coupled to dense map state |
| Viewport and interactions | `crates/web/src/canvas.rs`, `editor.rs`, `brush.rs` | Working pan, zoom, culling, paint and river interactions |
| Dense layers | `crates/core/src/layer.rs`, `schemas/map-layer-dense.schema.json` | Working typed dense storage and generic layer API |
| World storage | `crates/server/src/world_io.rs`, `world_transaction.rs`, `world_revision.rs` | Working filesystem, transaction, revision and recovery stack |
| Editor brushes | `crates/web/src/brush.rs`, `editor.rs` | Working elevation, land and river tools; domain-specific |
| Selection and inspector | `crates/web/src/editor.rs`, `crates/core/src/profile.rs` | Working cell/profile selection; bound to hex cell identity |
| Generator and Wizard | `crates/core/src/worldgen/`, `crates/server/src/build.rs`, `crates/web/src/wizard.rs` | Working pipeline through climate and water; many recipes remained experimental |
| Hydrology | `crates/core/src/worldgen/hydrology/` | Working drainage-first snapshot topology |
| Rivers and lakes | `crates/core/src/{rivers,lakes,river_pin,river_detach}.rs`, server routes | Working generated catalogs and author tools; compatibility projections remained |
| History | `crates/core/src/history.rs`, server/web `history.rs` | Working CoW states and divergence review; revisions were map-domain-specific |
| Schemas | `schemas/` | Working map, layer, river, lake and cell-profile contracts |
| Tests and fixtures | `fixtures/`, `tests/`, `crates/server/tests/` | Working regression, schema, API and browser fixtures |
| World scaffold | `toolchain/template/world/` | Working map-v2 world folder contract |
| Architecture docs | `docs/` | Normative map-v2 pipeline, API, revision, transaction and integrity documents |
| Agent helpers | `.cursor/skills/` | Map-v2-specific author and test guidance |

## Useful reference

- The transaction, revision, lock, recovery, and integrity patterns are useful
  examples of safe local writes, but their artifact graph must not be copied.
- The viewport input routing and mode-first shell separation show useful UI
  boundaries; geometry and renderer types are not reusable contracts.
- Hydrology snapshot tests demonstrate durable graph invariants, not a required
  model for the next spatial architecture.
- Seeded generator tests and failure-class guards are useful testing patterns.

## Do not carry forward by default

- Hex axial coordinates, `cell_id`, dense layer files, map manifests, or the
  map-v2 world scaffold.
- The land → geology → elevation → climate → hydrology pipeline order.
- Canvas renderer DTOs, editor brush types, or map-domain History revisions.
- Legacy compatibility projections, migrations, fixtures, and API paths.
- Any archived type merely because it already exists.

Reusing any archived contract requires a separate architecture decision.
