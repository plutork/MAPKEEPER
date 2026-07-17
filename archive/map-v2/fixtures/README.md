# Fixtures

Example data used by CI. Each contract has a `valid/` set (must pass its
schema) and an `invalid/` set (must fail it):

- `profiles/` -> `schemas/cell-profile.schema.json` (roadmap 1.2, D-12/D-22)
- `layers-dense/` -> `schemas/map-layer-dense.schema.json` (scale-layers, D-46)
- `manifests/` -> `schemas/map-manifest.schema.json` (D-36)

`layers-dense/invalid` covers the dense contract edges: a bad `states[i]` code,
an extra field, and a wrong `schema_version` must all fail.

Checked by `validate_schema.py` (`pip install jsonschema` then run it from
the repo root). The CLI query path itself (`init`/`profile set`/`get`/`list`,
plus `terrain set`/`get`) is covered separately by
`crates/cli/tests/query_flow.rs` (`cargo test`). Both run in CI:
`.github/workflows/ci.yml`.

## River dogfood worlds

`worlds/` — five Small elevation presets for river layer dogfood (see
`worlds/README.md`). Regenerate via `worlds/generate_fixture_worlds.py`.
Validated by `validate_schema.py` alongside schema fixtures.
