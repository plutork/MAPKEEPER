# World generation pipeline

Normative routing index for MAPKEEPER procedural world build. Shape spec: maintainer OS **D-56**; code layout: **D-92** (`crates/core/src/worldgen/`).

## Dependency order

```
land → coast / plates → geology → elevation → climate → hydrology
```

Upstream stages may read earlier outputs only. Spatial primitives (`hex`, `layer`, …) and I/O adapters (`server`, `web`, `cli`) stay outside `worldgen/`.

## Built wizard path (author UI)

| Wizard step | Pipeline stage | Core module |
|-------------|----------------|-------------|
| 1 Size & grid | bounds / manifest | `server/build.rs`, `core/build_state.rs` |
| 2 Land silhouette | `land_mask` | `worldgen/land/` |
| 3 Tectonics | `geology` | `worldgen/geology/` |
| 4 Elevation | elevation bridge | `worldgen/elevation/` |
| 5 Climate | temp / precip / ice | `worldgen/climate/` |
| 6 Water | lakes + rivers snapshot | `worldgen/hydrology/` |

Wizard UX contract: `docs/WORLD-GEN-UI.md` (D-57). English UI strings (D-58).

## D-56 eighteen-step spec (not all built)

Full tier list (T0–T7): meta → hex → land → geology → elevation → coast → climate → hydro → soils → biomes → resources/hazards/POI → validators.

**Shipped in product:** through **Water** (wizard step 6). **Not built:** soils, biomes, resources, validators — each needs `/idea` before code.

## Module map

Detailed file routing: `docs/CODEMAP-LITE.md` (worldgen/* entries).

| Stage | Path | Notes |
|-------|------|-------|
| land | `worldgen/land/` | D-104 split; silhouette + growth |
| coast | `worldgen/coast.rs` | auto `coast_distance` (D-90) |
| plates | `worldgen/plates.rs` | ephemeral Voronoi (D-87) |
| geology | `worldgen/geology/` | step 4 categorical |
| elevation | `worldgen/elevation/` | step 5 bridge (D-88/D-89) |
| climate | `worldgen/climate/` | step 5 wizard (D-90) |
| hydrology | `worldgen/hydrology/` | v2 snapshot topology (D-101) |

## Hygiene notes

- **land/** and **hydrology/** are intentionally large directory stages after D-92/D-104 modularization; further splits only via `/idea` Shape — not ad-hoc refactors.
- Regenerating an upstream step invalidates downstream generated layers (see `docs/OPS-INVARIANTS.md`).

## Related docs

- `docs/WORLD-GEN-UI.md` — wizard shell and draft lifecycle
- `docs/MAP-REVISION.md` — world revision / optimistic concurrency
- `docs/OPS-INVARIANTS.md` — mutating ops and safe sequences for agents
