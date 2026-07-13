# World transaction I/O (agent-reliability transactional-io)

Multi-file world mutations use `WorldMutationPlan` in `crates/server/src/world_transaction.rs`.

This is **not** a database and **not** a claim of absolute multi-file filesystem atomicity.

## API

| Step | What happens |
|------|----------------|
| `begin(world_path)` | Recover orphan txns; create `{world}/.mapkeeper-staging/{txn_id}/` |
| `stage_write` / `stage_delete` | Backup active bytes → `backups/`; new payloads → `staged/` |
| `validate_staged` | Optional caller checks before commit |
| `commit()` | `txn.json` journal → apply staged ops → post-commit hooks → remove txn dir |
| Error during commit | Restore all targets from backups (in-process rollback) |

Entry point for bundles: build a plan, stage all targets, then `commit()`.

## Migrated bundles (phase 1)

- `persist_rivers` — catalog + `river_id` layer
- `persist_lake_generation` — lakes + lake layer + cleared rivers + deletes
- `persist_land_mask_bundle` — land_mask + elevation
- `persist_climate_layers_bundle` — temperature + precipitation + ice

Not migrated: single-file `write_dense_layer`, `persist_lakes`, `reset_build_bounds`, `persist_hydrology_snapshot` (keeps its own rename chain).

## Guarantees

### Normal handler error (in-process)

- Active files are either **fully at the pre-txn bytes** (rollback) or **fully at the committed bytes**.
- Post-commit hooks (`invalidate_hydrology_snapshot`) run only after all staged file ops succeed.
- No partial bundle: if commit fails mid-sequence, backups restore every target in the plan.

### Process crash

| `txn.json` status | Recovery on next `begin` / server start |
|-------------------|----------------------------------------|
| `staging` | Active never changed; txn dir deleted |
| `committing` | **Full rollback** from `backups/` for every target, then txn dir deleted |

The `committing` journal exists because a crash between individual file writes could leave a mixed bundle; recovery chooses **restore all backups** (safe, not forward-complete).

### Not guaranteed

- **Power loss / OS/filesystem failure** during a single `write()` or `rename()` — a file may be truncated or half-written despite rollback logic.
- **Cross-process** coordination — only one server process per machine is assumed; no distributed lock.
- **Map revision** — not part of this helper until revision contract is approved.

## Post-commit contract

Declared per plan via `post_commit_invalidate_hydrology()`:

- Runs after all file targets commit.
- Failure after file commit but before hook completes may leave files updated without invalidation — treated as a handler error (rollback runs on hook failure during commit).

## Startup

`build_router` calls `recover_all_registered_worlds()` to discard/recover orphan `.mapkeeper-staging` dirs.

## Related

- Per-world write serialization: `docs/WORLD-SCOPE-API.md` (world-lock section)
- Scope header: `X-World-Id`
