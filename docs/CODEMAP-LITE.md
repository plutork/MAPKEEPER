# MAPKEEPER Code Map — Lite

Active product-shell routing (ownership index). Product invariants live in
`README.md`. Generated symbol map: `docs/CODEMAP.md` (`scripts/gen_codemap.py`).

| Need | Start here |
|---|---|
| World⊃maps layout (N-035/N-037): world `mapkeeper.toml` + `maps/<id>/map.toml` | `crates/core/src/world.rs` · `crates/server/src/world_layout.rs` |
| Projects registry shape | `crates/core/src/projects.rs` |
| Spatial foundation + relief field | `crates/core/src/spatial/` |
| Hard-disk brush + Airbrush rate | `crates/core/src/spatial/brush.rs` |
| Extent presets + Create catalog | `crates/core/src/spatial/presets.rs` |
| World ↔ grid ↔ screen conversions | `crates/core/src/spatial/convert.rs` |
| Health and static server | `crates/server/src/lib.rs` |
| Projects routes/handlers | `crates/server/src/projects.rs` |
| Create / Delete sagas (N-031) | `crates/server/src/projects/create.rs` · `projects/delete.rs` |
| Create map-preset cards API | `crates/server/src/presets.rs` |
| Spatial routes + view | `crates/server/src/spatial.rs` |
| Spatial durable IO / stroke engine (N-031) | `crates/server/src/spatial/persist.rs` · `spatial/stroke.rs` |
| Atomic replace + bak classify | `crates/server/src/atomic_io.rs` |
| Per-world locks + stroke staging | `crates/server/src/state.rs` |
| World path identity + registry | `crates/server/src/world_io.rs` |
| Delete recovery: inflight, reconcile, trash | `crates/server/src/world_io/delete_recovery.rs` |
| Registry recovery: restore bak, quarantine (N-025) | `crates/server/src/world_io/registry_recovery.rs` → `POST /api/projects/restore-bak` |
| Corruption recovery offered to author (N-025) | `crates/web/shell-math.js` `bakRestoreOffer` · `worlds.js` |
| Server tests (outside implementation, N-031) | `crates/server/src/projects/tests.rs` · `crates/server/src/spatial/tests.rs` · `crates/server/src/world_io/tests.rs` |
| Thin shell document | `crates/web/index.html` + `styles.css` + `main.js` |
| Sole Editor/view state owner | `crates/web/workspace-state.js` |
| CRS renderer / camera / relief / stroke client | `crates/web/renderer.js` · `camera.js` · `relief-tool.js` · `spatial-transaction.js` (overscan offscreen cache + viewport blit; rebuild outside margin; zoom debounce + seamless cache swap) |
| Continuous stroke ACK (N-039) | server `spatial.rs` / `spatial/stroke.rs` / `spatial/persist.rs` → delta `{revision, applied_cells, server_timings}`; web `applyStrokeAck` + `commitDirtyMapCache` preserves centers/heights/unaffected raster |
| View Reset zoom (contain to host) | `#reset-zoom` → `fitCamera`; `shell-math.js` `fitZoomForViewport` |
| Camera sticky fit on resize (N-029) | `workspace-state.js` `cameraFollowsFit` · `camera.js` `observeCanvasHost` · `shell-math.js` `nextCameraFollowsFit` |
| Pure brush geometry / hover readout | `crates/web/brush-geometry.js` · `hover-readout.js` |
| Pure shell math (unit-tested) | `crates/web/shell-math.js` |
| Relief gesture rules (domain, N-030/N-038) | `crates/core/src/spatial/field.rs` `next_relief_value` / `next_relief_absolute` / `smooth_relief_average` → `probe_next_relief*` |
| Mirrored threshold parity gate (N-030) | `scripts/check_domain_constants.py` |
| Bench hooks (`?bench=1` only) | `crates/web/bench-hooks.js` |
| Home worlds → maps L2 + Add map / Create / Delete | `crates/web/worlds.js` · `api.js` · `index.html` |
| WASM bootstrap + pick helpers | `crates/web/src/lib.rs` · `wasm-api.js` |
| Desktop launch | `crates/desktop/src/lib.rs` |
| Local checks | `scripts/check.ps1` |
| Archive isolation guard | `scripts/check_archive_isolation.py` |
| Doc drift | `scripts/check_doc_drift.py` |
| Headless spatial smoke | `scripts/smoke-headless.ps1` |
| Relief render scale bench | `scripts/bench-render-scale.mjs` + `docs/perf/` |
| Mature-map authoring bench (N-039) | `scripts/bench-authoring-performance*.mjs` · `crates/web/bench-hooks.js` · `docs/perf/large-map-authoring-report.json` |

`archive/map-v2/` is research material only — not an active routing target.
See `archive/map-v2/README.md`.
