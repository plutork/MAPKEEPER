# World-scoped API (agent-reliability)

## Canonical identity

| Field | Role |
|-------|------|
| **`world_id`** | Opaque string from `mapkeeper.toml` `[world].id` — **only** client-supplied world selector on map APIs |
| **Server path** | Resolved from maintainer `projects.json` registry — **never** accepted on mutating map routes |

## Scope header

```
X-World-Id: <world_id>
```

Resolution:

1. Validate `world_id` format (`world::is_valid_world_id`).
2. Lookup in server `projects.json` by `id`.
3. Normalize registry path; verify `mapkeeper.toml` manifest id matches.
4. Return `{ id, path }` for handler I/O.

## Endpoint classes

| Class | Scope rule (current) |
|-------|----------------------|
| **Projects** (`/api/projects*`, `/api/fixture-worlds*`) | Manage registry + UI `active` — unchanged |
| **Read** (`GET /api/map`, layers, rivers, lakes, diagnostics, profiles) | `X-World-Id` **or** legacy `active` fallback |
| **Mutate** (PUT/POST/DELETE on map/build/water/layers) | `X-World-Id` **or** legacy `active` fallback (migration window) |

## Migration window

1. **Now:** Web/desktop send `X-World-Id` on every scoped call after open/create.
2. **Server:** Mutating routes still accept missing header when `active` is set (harness/older clients).
3. **Later:** Mutating routes return `409` without `X-World-Id` (after clients migrated).

`POST /api/projects/close` clears **UI active only** — it does not invalidate scoped requests that carry `X-World-Id`.

## Security

- No arbitrary filesystem paths on map/build/water/layer routes.
- Registry path is server-owned; manifest id mismatch → `409`.
- Path normalization uses existing `normalize_world_path` (canonicalize where possible).

## Per-world write lock (agent-reliability world-lock)

- Key: canonical `world_id` (same as `X-World-Id`).
- Mutating handlers acquire `WorldWriteGuard` before any world folder RMW.
- Different `world_id` values write in parallel; same `world_id` serializes.
- `AppState` mutex is held only briefly for `active` UI; not during filesystem I/O.
- Lock ordering: world guard → existing `world_io` bundle order (manifest/catalog before layers).
- Migration window for scope unchanged; lock is always on (no disable flag).

## Transactional multi-file I/O (agent-reliability transactional-io)

- `WorldMutationPlan`: stage → validate → commit with in-process rollback.
- Not claimed as absolute multi-file atomicity — see `docs/WORLD-TRANSACTION-IO.md` for error vs crash vs power-loss guarantees.
- Migrated bundles: `persist_rivers`, `persist_lake_generation`, land-mask, climate.
- Orphan `.mapkeeper-staging` recovered on server start.

- `session_id`, map revision, authentication.
