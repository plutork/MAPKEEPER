# Map revision — optimistic concurrency (agent-reliability)

Coarse world-level revision for alpha optimistic concurrency. Extensible later to per-artifact revision.

## Authoritative storage

`map/manifest.json` field `revision: u64` (monotonic, starts at `0`).

- Missing field on disk → treated as `0` (`#[serde(default)]`).
- Bump writes the whole manifest JSON (same file as bounds/layers index).
- **Only** updated after a successful map mutation commit — never on failed/rolled-back ops.

## Operations that bump revision

Any successful **map-scoped** write under the world folder:

| Area | Routes / helpers |
|------|------------------|
| Layers | `PUT /api/layers/:id/batch`, `PUT /api/layers/:id/cells/:q/:r` |
| Profiles | `PUT /api/cells/:q/:r/profile` |
| Build wizard | `PUT /api/build`, `PUT /api/build/bounds`, land-mask/geology/elevation/climate generate, `PUT /api/build/land-mask/cells` |
| Rivers | `PUT /api/rivers`, pin/append/pop/delete/detach/generate |
| Lakes | `PUT /api/lakes`, `POST /api/lakes/generate` |
| Transaction bundles | `persist_rivers`, `persist_lakes`, `persist_lake_generation`, `persist_land_mask_bundle`, `persist_climate_layers_bundle` |

Hydrology snapshot activation (`persist_hydrology_snapshot`) uses its own fingerprint contract today; coarse map revision bumps when invoked from a map mutation path that already commits via txn.

## Operations that do **not** bump revision

- All `GET` (map, layers, diagnostics, integrity, projects list)
- `POST /api/projects/open|close|forget` (registry only)
- Failed pre-commit integrity, failed txn rollback, simulated failpoints
- `create_project` scaffold (initial `revision: 0`; first map write bumps to `1`)

## API contract

**Request:** mutating calls send `base_revision` via:

1. Header `X-World-Base-Revision` (preferred), or
2. JSON field `base_revision` in the body (overrides header when both present)

**Legacy migration (no silent LWW):**

| Client sends | Current revision | Result |
|--------------|------------------|--------|
| `base_revision` matches | any | proceed |
| `base_revision` mismatch | any | `409` + `current_revision` |
| omitted | `0` | allowed once (bootstrap legacy worlds) |
| omitted | `> 0` | `428 Precondition Required` + `current_revision` |

**Success:** response includes `result_revision` (JSON field and/or header `X-World-Result-Revision`).

**Conflict body:**
```json
{ "current_revision": 3, "conflict_kind": "world_revision_mismatch" }
```

## Multi-process / same world path

**Not supported.** One server process per world folder is the alpha contract.

- In-process `WorldLockManager` serializes writes per `world_id`.
- Two `mapkeeper-server` (or Tauri) instances pointing at the same folder can corrupt data; revision does not coordinate across processes.
- Authors/agents: one launcher per world path.

## Related

- `docs/WORLD-SCOPE-API.md` — `X-World-Id`
- `docs/WORLD-TRANSACTION-IO.md` — commit before bump
