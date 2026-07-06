# Fixtures

Example data used by CI. Each contract has a `valid/` set (must pass its
schema) and an `invalid/` set (must fail it):

- `profiles/` -> `schemas/cell-profile.schema.json` (roadmap 1.2, D-12/D-22)
- `layers/` -> `schemas/map-layer.schema.json` (Hex Map Model Foundation, D-36)
- `manifests/` -> `schemas/map-manifest.schema.json` (D-36)

`layers/invalid` covers the partial-state contract too: a stored `unknown`
state (`unknown = missing key`, never on disk) and a `value` entry with no
`value` must both fail.

Checked by `validate_schema.py` (`pip install jsonschema` then run it from
the repo root). The CLI query path itself (`init`/`profile set`/`get`/`list`,
plus `terrain set`/`get`) is covered separately by
`crates/cli/tests/query_flow.rs` (`cargo test`). Both run in CI:
`.github/workflows/ci.yml`.
