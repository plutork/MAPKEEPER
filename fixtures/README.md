# Fixtures

Example cell profiles used by CI (roadmap 1.2 V0-done criterion, D-12):

- `profiles/valid/*.json` — must pass `schemas/cell-profile.schema.json`
- `profiles/invalid/*.json` — must fail it (missing required field, bad
  `cell_id` pattern, unknown property)

Checked by `validate_schema.py` (`pip install jsonschema` then run it from
the repo root). The CLI query path itself (`init`/`profile set`/`get`/`list`)
is covered separately by `crates/cli/tests/query_flow.rs` (`cargo test`).
Both run in CI: `.github/workflows/ci.yml`.
