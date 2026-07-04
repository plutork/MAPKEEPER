# Schemas

JSON Schema contracts for mapkeeper data — cell profiles, `mapkeeper.toml`.

## `cell-profile.schema.json`

V0 cell profile fields (roadmap 3.2, decision D-22) — mirrors
`crates/core/src/profile.rs::CellProfile`:

| Field | Required | Role |
|-------|----------|------|
| `cell_id` | yes | canonical key `{world_id}.hex.q{q}.r{r}` |
| `display_name` | yes | human-readable place name |
| `slug` | no | short id for agents — not the primary key |
| `notes` | no (default `""`) | free text stub for V0 |

Deliberately **not** in V0: tags/category, `created_at`/`updated_at` (Later —
roadmap block 6, time slices). See the `mapkeeper-cell-schema` skill.

**`mapkeeper.toml` schema:** not yet written — no format instability reported
so far; add when it starts drifting or CI needs it.
