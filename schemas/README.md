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

## `map-layer-dense.schema.json` (D-46, scale-layers)

The **only** map-state layer format, `map/layers/<id>.json` — mirrors
`crates/core/src/layer.rs::DenseLayer`. Layers are machine-readable world state,
kept separate from author profiles. Dense and **index-addressed** for the
50–100k ceiling: cells are addressed by linear index within the map bounds
(`core::hex::MapBounds::index_of`), not by `cell_id` strings, and categorical
values are palette/dictionary-encoded. `cell_id` stays the external identity
(API / profiles / agent); the linear index is the internal storage key.

| Field | Meaning |
|-------|---------|
| `schema_version` | `2` |
| `states[i]` | `0`=unknown, `1`=none, `2`=value |
| `palette` + `codes[i]` | categorical: dictionary + per-cell palette index |
| `values[i]` | integer value column |

Partial-state trio (D-36) is preserved: `unknown` (not filled), `none`
(explicitly absent), `value` (concrete). Layer files are created on first write
sized to the map bounds; a `GET` of an absent layer returns an empty typed
layer. The old sparse `map-layer.schema.json` (v1) was removed once server, CLI
and web switched to dense (`scale-layers--adapters`).

## `map-manifest.schema.json` (D-36)

`map/manifest.json` — bounds + declared layers; mirrors
`crates/core/src/layer.rs::MapManifest`. V0 bounds: `hex-radius` only.

**`mapkeeper.toml` schema:** not yet written — no format instability reported
so far; add when it starts drifting or CI needs it.
