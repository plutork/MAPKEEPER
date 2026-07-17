# World integrity checker (agent-reliability)

Pure validation in `mapkeeper-core` with read-only adapters in server/CLI.

## Modes

| Mode | When | Side effects |
|------|------|--------------|
| **PreCommit** | `WorldMutationPlan::commit()` before writes | Fails commit; rolls back staging (existing txn helper) |
| **PostCommit** | `GET /api/integrity`, `mapkeeper integrity check` | None — audit only; **does not** undo committed files |

Post-commit audit reports problems but never pretends to roll back a finished transaction.

## Checks (v1)

1. `rivers.json` ↔ `river_id` layer (`rivers.catalog_layer_mismatch`)
2. `lakes.json` ↔ `lake_id` layer
3. `manifest.json` bounds ↔ dense layer lengths
4. Hydrology snapshot fingerprint / base revision currentness
5. `named-rivers.json` segment refs ↔ hydrology snapshot catalog
6. Required world files (`mapkeeper.toml`, `map/manifest.json`) and JSON/TOML parse errors

No auto-fix in v1.

## Machine-readable report

`IntegrityReport` + `IntegrityFinding` in `crates/core/src/integrity.rs`:

- Stable `code` strings (e.g. `rivers.catalog_layer_mismatch`, `layer.bounds_length_mismatch`)
- `severity`: `error` | `warning` | `info`
- Optional `detail` for agents

## Agent surfaces

- **HTTP:** `GET /api/integrity` with explicit world scope (`X-World-Id` header, same as other scoped routes)
- **CLI:** `mapkeeper integrity check --world <path>` — prints JSON, exit code `1` on errors

## Fixtures

- `fixtures/worlds/gentle-plain` — valid; no false-positive errors
- `fixtures/worlds/integrity-river-mismatch` — deliberate `rivers.json` / `river_id` mismatch for regression

## Related

- Transaction staging: `docs/WORLD-TRANSACTION-IO.md`
- World scope header: `docs/WORLD-SCOPE-API.md`
