# Profiles

Queryable cell profiles for Cursor agents — same data the author sees in the editor.

One JSON file per cell, named `{world_id}.hex.q{q}.r{r}.json`. V0 fields:

- `cell_id` (required) — canonical `{world_id}.hex.q{q}.r{r}`, matches the filename
- `display_name` (required) — human-readable place name; empty is valid ("painted, unnamed yet")
- `slug` (optional) — short id for agents, not the primary key
- `notes` (optional, default `""`) — free text; canon fields/format are Later

The editor validates this on save — you do not need to hand-check it. Formal JSON Schema (for tooling outside the editor): [`schemas/cell-profile.schema.json`](https://github.com/plutork/MAPKEEPER/blob/main/schemas/cell-profile.schema.json) in the mapkeeper repo.
