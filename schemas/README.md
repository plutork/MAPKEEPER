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

Profiles are **author-facing** description and are **not** a map layer — map
state lives separately under `map/` (below).

## `map-layer.schema.json` (D-36)

One map-state layer file, `map/layers/<id>.json` — mirrors
`crates/core/src/layer.rs::Layer`. Layers are machine-readable world state,
kept separate from author profiles; `cell_id` is the shared anchor.

Sparse `cell_id -> entry`; **a missing key means the cell is `unknown`** for
that layer (partial-state model):

| On disk | Meaning |
|---------|---------|
| key absent | `unknown` — not filled / not decided |
| `{ "state": "none" }` | `none` — explicitly absent |
| `{ "state": "value", "value": <T> }` | `value` — concrete known value |

V0 proof layer: `terrain` (`value_type: "categorical"`, string values). Other
layers (elevation, water, …) are additive later.

## `map-manifest.schema.json` (D-36)

`map/manifest.json` — bounds + declared layers; mirrors
`crates/core/src/layer.rs::MapManifest`. V0 bounds: `hex-radius` only.

**`mapkeeper.toml` schema:** not yet written — no format instability reported
so far; add when it starts drifting or CI needs it.
