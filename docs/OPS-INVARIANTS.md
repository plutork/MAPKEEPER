# OPS-INVARIANTS — operational contract for MAPKEEPER agents

Operational map for **external agents** and maintainers: which artifacts are authoritative, how writes bundle and invalidate each other, and safe recovery — without reading all of `crates/server/`.

**Machine-readable source of truth:** `schemas/agent_ops_registry.json`

**Do not duplicate here:** full module/route lists (see generated tables below). Code navigation stays in `docs/CODEMAP-LITE.md` → `docs/CODEMAP.md`.

## Related contracts

| Topic | Doc |
|-------|-----|
| World scope header | `docs/WORLD-SCOPE-API.md` |
| Optimistic concurrency | `docs/MAP-REVISION.md` |
| Multi-file commits | `docs/WORLD-TRANSACTION-IO.md` |
| Validation codes | `docs/INTEGRITY-CHECKER.md` |
| Mutating request logs | `crates/server/src/op_log.rs` (`X-Request-Id`, `operation_id`) |

---

## Authoritative vs derived artifacts

**Authoritative** — agent edits these (directly or via API); server persists them as source bytes.

- `mapkeeper.toml` — world id; optional `[build]` wizard draft
- `map/manifest.json` — bounds, layer index, **coarse `revision`**
- `map/layers/{layer_id}.json` — dense layers (elevation, land_mask, geology, …)
- `map/rivers.json`, `map/lakes.json`, `map/named-rivers.json` — water catalogs / bindings
- `map/hydrology-v2.json` — hydrology v2 snapshot (treated authoritative once activated; invalidated when base inputs change)
- `profiles/{cell_id}.json` — per-cell author metadata

**Derived** — server recomputes or projects; do not hand-edit to “fix” desync.

- `map/layers/river_id.json` — projection from `rivers.json` or hydrology activation
- `map/layers/lake_id.json` — projection from `lakes.json`
- Hydrology channel/segment views inside `hydrology-v2.json` — invalidated when elevation, lakes, precip, or land_mask inputs change

**Server registry (not in world folder):** `projects.json` — launcher list + paths; mutated only by `/api/projects*` and `/api/fixture-worlds/open`.

---

## Operation bundles (concept)

A **bundle** is the set of files a single HTTP operation commits together. Bundles use one of:

| Txn kind | Guarantee (in-process) |
|----------|-------------------------|
| `world_transaction` | Stage → pre-commit integrity → commit → post-commit hooks; rollback on error |
| `mutate_map` | Revision check → single-file or small direct write → revision bump |
| `rename_chain` | Hydrology snapshot activation (own staged rename + river_id projection) |
| `direct` | Registry or scaffold writes outside map revision |

See generated **Operation bundles** table for file lists.

---

## Invalidation graph (concept)

When base hydrology inputs change, **`map/hydrology-v2.json` must be treated stale** until `POST /api/rivers/generate` runs again.

Typical triggers:

1. **Land / elevation / climate** wizard or layer writes touching `land_mask`, `elevation`, `precipitation`, `lake_id`
2. **River or lake catalog** writes (`PUT /api/rivers`, `PUT /api/lakes`, legacy river edits)
3. **Lake generation** — also clears `rivers.json` and deletes active hydrology snapshot

Generated **Invalidation graph** table lists registry edges. After invalidation, `GET /api/rivers` reports `read_only: true` when no valid snapshot; legacy pin/detach/brush paths return **409**.

---

## World scope and `base_revision`

| Scope | Header | Revision |
|-------|--------|----------|
| `registry` | none (paths in JSON body) | not used |
| `world_mutate` | `X-World-Id` (or legacy `active` fallback) | see below |

**`base_revision`** (map-scoped mutates only):

1. Prefer header `X-World-Base-Revision`; body `base_revision` overrides when both sent
2. `revision == 0` on disk → omitted base allowed **once** (bootstrap)
3. `revision > 0` → omitted base → **428** `base_revision_required`
4. Mismatch → **409** `world_revision_mismatch` + `current_revision`
5. Success → `X-World-Result-Revision` / JSON `result_revision`

Always send `X-World-Id` for parallel clients; do not rely on `POST /api/projects/open` alone when multiple worlds are registered.

---

## Conflicting operations

All **`world_mutate`** operations on the **same `world_id`** conflict: they serialize under `WorldWriteGuard` (one mutator at a time per world).

They also share **one coarse `revision`**: overlapping writes without fresh `base_revision` produce **409**.

**Cross-world:** different `X-World-Id` values may run in parallel.

**Registry ops** (`post.projects*`, `post.fixture_worlds.open`) do not take `base_revision`; they can run while a world mutates **another** world, but `post.projects.delete` must not target a world with in-flight writes.

**Hydrology vs legacy rivers:** while `hydrology-v2.json` is valid, `post.rivers.pin`, `post.rivers.detach`, and related legacy edits are **rejected** — not a revision conflict; do not retry with new revision.

---

## Safe sequences for an external agent

Minimal happy path after opening a world:

1. `POST /api/projects/open` (or create) — register path; note `world_id`
2. `GET /api/map` with `X-World-Id` — read `revision` (and bounds)
3. For each mutate: attach `X-World-Id`, `X-World-Base-Revision: <last known>`, optional `X-Request-Id`
4. On success: store `X-World-Result-Revision` for the next call
5. Before water edits: `GET /api/hydrology/diagnostics` or `GET /api/integrity` if unsure about snapshot validity

**Pipeline order (wizard / generation):** bounds → land_mask → geology → elevation → climate → lakes generate → rivers generate. Skipping steps leaves downstream inputs empty or default.

**Do not interleave** unrelated mutates on the same world from multiple agents without merging at revision boundaries — there is no CRDT; use reload + single-writer or explicit revision handoff.

**Read-only audit:** `GET /api/integrity`, `GET /api/hydrology/diagnostics` — no lock, no revision.

---

## Recovery

### HTTP 409 `world_revision_mismatch`

1. Read `current_revision` from response body
2. `GET` relevant artifacts (`/api/map`, layer, rivers, lakes) to refresh local state
3. Reconcile intent (merge by cell id / catalog id, or abandon stale edit)
4. Retry **once** with `X-World-Base-Revision: current_revision`
5. If second 409 — stop; another writer is active; use logs (`X-Request-Id`) to correlate

### Failed transaction (`500` / `operation_failed` in op log)

- In-process rollback restores pre-txn bytes for bundled writes (`world_transaction` bundles)
- Revision **not** bumped
- Retry from step 2 above; if repeated, `GET /api/integrity` before further writes

### Pre-commit integrity rejection

- Commit aborted; no file changes; revision unchanged
- Fix underlying catalog/layer mismatch (often stale hydrology vs catalog)
- May require invalidate + regen: `POST /api/lakes/generate` then `POST /api/rivers/generate`, or fix catalog via `PUT`

### Post-commit integrity findings (`GET /api/integrity`)

- Audit only — does **not** roll back committed files
- Treat `severity: error` as blocking further mutates until resolved

### Orphan staging (crash)

- Server startup / `build_router` recovers `.mapkeeper-staging` — see `docs/WORLD-TRANSACTION-IO.md`

---

<!-- GENERATED:BEGIN -->
## Generated reference (do not edit by hand)

_Source: `schemas/agent_ops_registry.json` · regenerate: `python scripts/gen_ops_invariants.py`_

### Artifacts

| Kind | ID | Path | Role |
|------|-----|------|------|
| authoritative | world_manifest | `mapkeeper.toml` | World identity, optional [build] draft |
| authoritative | map_manifest | `map/manifest.json` | Bounds, layer index, coarse revision |
| authoritative | dense_layer | `map/layers/{layer_id}.json` | Authoritative dense layer payload |
| authoritative | rivers_catalog | `map/rivers.json` | Legacy/manual river catalog |
| authoritative | lakes_catalog | `map/lakes.json` | Lake catalog |
| authoritative | named_rivers | `map/named-rivers.json` | Author-named river bindings to hydrology segments |
| authoritative | hydrology_snapshot | `map/hydrology-v2.json` | Hydrology v2 snapshot (derived inputs fingerprinted) |
| authoritative | cell_profile | `profiles/{cell_id}.json` | Per-cell author profile (display_name, notes) |
| derived | river_id_layer | `map/layers/river_id.json` | from rivers_catalog; Dense projection of river catalog or hydrology activation |
| derived | lake_id_layer | `map/layers/lake_id.json` | from lakes_catalog; Dense projection of lake catalog |
| derived | hydrology_render | `map/hydrology-v2.json` | from elevation,lakes,precip,land_mask; Channel graph + segments (invalidated when base inputs change) |
| server | projects_registry | `server:projects.json` | Launcher registry (not inside world folder) |

### Operation bundles

| Bundle | Txn | Writes | Deletes | Invalidates |
|--------|-----|--------|---------|-------------|
| `project_create` | direct | mapkeeper.toml, map/manifest.json, map/layers/elevation.json | — | — |
| `registry_entry` | direct | server:projects.json | — | — |
| `build_draft` | mutate_map | mapkeeper.toml | — | — |
| `build_bounds_reset` | mutate_map | map/manifest.json, map/layers/* | — | — |
| `land_mask_bundle` | world_transaction | map/layers/land_mask.json, map/layers/elevation.json | — | hydrology_snapshot |
| `geology_layer` | mutate_map | map/layers/geology.json | — | — |
| `elevation_layer` | mutate_map | map/layers/elevation.json | — | hydrology_snapshot |
| `climate_bundle` | world_transaction | map/layers/temperature.json, map/layers/precipitation.json, map/layers/ice.json | — | hydrology_snapshot |
| `single_dense_layer` | mutate_map | map/layers/{layer_id}.json | — | hydrology_snapshot |
| `cell_profile` | mutate_map | profiles/{cell_id}.json | — | — |
| `rivers_catalog_layer` | world_transaction | map/rivers.json, map/layers/river_id.json | — | hydrology_snapshot |
| `lakes_catalog_layer` | world_transaction | map/lakes.json, map/layers/lake_id.json | — | hydrology_snapshot |
| `lake_generation` | world_transaction | map/lakes.json, map/layers/lake_id.json, map/rivers.json, map/layers/river_id.json | map/hydrology-v2.json | hydrology_snapshot |
| `hydrology_activate` | rename_chain | map/hydrology-v2.json, map/layers/river_id.json | — | — |
| `legacy_river_edit` | mutate_map | map/rivers.json, map/layers/river_id.json | — | hydrology_snapshot |

### Invalidation graph

| Trigger | When | Invalidates |
|---------|------|-------------|
| `land_mask_bundle` | always | `hydrology_snapshot` |
| `elevation_layer` | always | `hydrology_snapshot` |
| `climate_bundle` | always | `hydrology_snapshot` |
| `single_dense_layer` | layer_id in land_mask,elevation,lake_id,precipitation | `hydrology_snapshot` |
| `rivers_catalog_layer` | always | `hydrology_snapshot` |
| `lakes_catalog_layer` | always | `hydrology_snapshot` |
| `lake_generation` | always | `hydrology_snapshot`, `rivers_catalog` |
| `legacy_river_edit` | always | `hydrology_snapshot` |

### Mutating operations

| Method | Path | operation_id | Scope | base_revision | Bundle | Reads | Writes | Invalidates | Conflicts |
|--------|------|----------------|-------|---------------|--------|-------|--------|-------------|-----------|
| POST | `/api/projects` | `post.projects` | registry | never | `project_create` | — | mapkeeper.toml, map/manifest.json, map/layers/elevation.json, server:projects.json | — | per_world_map_write |
| POST | `/api/projects/open` | `post.projects.open` | registry | never | `registry_entry` | server:projects.json | server:projects.json | — | — |
| POST | `/api/projects/forget` | `post.projects.forget` | registry | never | `registry_entry` | server:projects.json | server:projects.json | — | — |
| POST | `/api/projects/delete` | `post.projects.delete` | registry | never | `registry_entry` | server:projects.json | server:projects.json | — | per_world_map_write |
| POST | `/api/projects/close` | `post.projects.close` | registry | never | `registry_entry` | server:projects.json | server:projects.json | — | — |
| POST | `/api/fixture-worlds/open` | `post.fixture_worlds.open` | registry | never | `registry_entry` | server:projects.json | server:projects.json | — | — |
| PUT | `/api/build` | `put.build.state` | world_mutate | required_after_first_bump | `build_draft` | mapkeeper.toml | mapkeeper.toml | — | per_world_map_write |
| PUT | `/api/build/bounds` | `put.build.bounds` | world_mutate | required_after_first_bump | `build_bounds_reset` | map/manifest.json, map/layers/* | map/manifest.json, map/layers/* | — | per_world_map_write |
| POST | `/api/build/land-mask/generate` | `post.build.land_mask.generate` | world_mutate | required_after_first_bump | `land_mask_bundle` | map/manifest.json | map/layers/land_mask.json, map/layers/elevation.json | hydrology_snapshot | per_world_map_write |
| PUT | `/api/build/land-mask/cells` | `put.build.land_mask.cells` | world_mutate | required_after_first_bump | `land_mask_bundle` | map/layers/land_mask.json, map/layers/elevation.json | map/layers/land_mask.json, map/layers/elevation.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/build/geology/generate` | `post.build.geology.generate` | world_mutate | required_after_first_bump | `geology_layer` | map/manifest.json, map/layers/land_mask.json | map/layers/geology.json | — | per_world_map_write |
| POST | `/api/build/elevation/generate` | `post.build.elevation.generate` | world_mutate | required_after_first_bump | `elevation_layer` | map/layers/land_mask.json, map/layers/geology.json | map/layers/elevation.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/build/climate/generate` | `post.build.climate.generate` | world_mutate | required_after_first_bump | `climate_bundle` | map/layers/elevation.json, map/layers/land_mask.json | map/layers/temperature.json, map/layers/precipitation.json, map/layers/ice.json | hydrology_snapshot | per_world_map_write |
| PUT | `/api/cells/:q/:r/profile` | `put.cells.profile` | world_mutate | required_after_first_bump | `cell_profile` | profiles/{cell_id}.json | profiles/{cell_id}.json | — | per_world_map_write |
| PUT | `/api/layers/:id/batch` | `put.layers.batch` | world_mutate | required_after_first_bump | `single_dense_layer` | map/layers/{layer_id}.json, map/manifest.json | map/layers/{layer_id}.json | hydrology_snapshot | per_world_map_write |
| PUT | `/api/layers/:id/cells/:q/:r` | `put.layers.cell` | world_mutate | required_after_first_bump | `single_dense_layer` | map/layers/{layer_id}.json | map/layers/{layer_id}.json | hydrology_snapshot | per_world_map_write |
| PUT | `/api/rivers` | `put.rivers` | world_mutate | required_after_first_bump | `rivers_catalog_layer` | map/rivers.json, map/manifest.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/rivers/pin` | `post.rivers.pin` | world_mutate | required_after_first_bump | `legacy_river_edit` | map/rivers.json, map/layers/elevation.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/rivers/:id/detach` | `post.rivers.detach` | world_mutate | required_after_first_bump | `legacy_river_edit` | map/rivers.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/rivers/append` | `post.rivers.append` | world_mutate | required_after_first_bump | `legacy_river_edit` | map/rivers.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/rivers/:id/pop` | `post.rivers.pop` | world_mutate | required_after_first_bump | `legacy_river_edit` | map/rivers.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| DELETE | `/api/rivers/:id` | `delete.rivers.id` | world_mutate | required_after_first_bump | `legacy_river_edit` | map/rivers.json | map/rivers.json, map/layers/river_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/rivers/generate` | `post.rivers.generate` | world_mutate | required_after_first_bump | `hydrology_activate` | map/layers/elevation.json, map/layers/lake_id.json, map/layers/precipitation.json, map/layers/land_mask.json | map/hydrology-v2.json, map/layers/river_id.json | — | per_world_map_write |
| PUT | `/api/lakes` | `put.lakes` | world_mutate | required_after_first_bump | `lakes_catalog_layer` | map/lakes.json, map/manifest.json | map/lakes.json, map/layers/lake_id.json | hydrology_snapshot | per_world_map_write |
| POST | `/api/lakes/generate` | `post.lakes.generate` | world_mutate | required_after_first_bump | `lake_generation` | map/layers/elevation.json, map/layers/precipitation.json | map/lakes.json, map/layers/lake_id.json, map/rivers.json, map/layers/river_id.json | hydrology_snapshot, rivers_catalog | per_world_map_write |

### Conflict groups

- **`per_world_map_write`** (world_mutate): All operations in this group serialize on the same world_id (WorldWriteGuard).

<!-- GENERATED:END -->
