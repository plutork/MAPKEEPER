# mapkeeper

Generic **local** world editor built for the age of AI agents.

The map is not only a picture — it is the interface to a machine-readable world.

## Create your world

**Now (early access):** [GitHub template](https://github.com/plutork/mapkeeper-world-template/generate) — for authors comfortable with git.

**Target UX (V0):** open mapkeeper → **New world** — pre-configured agents and folders, no git required.

Details: [toolchain/template/README.md](toolchain/template/README.md).

## Docs

- **[STARTER_PACK.md](STARTER_PACK.md)** — product pitch, invariants, milestones.

## This repo (product)

Clone MAPKEEPER when you work on **mapkeeper itself** (editor, schemas, contracts) — not when you write lore.

Maintainers and contributors: see source and [toolchain/](toolchain/).

### Repository layout

Cargo workspace, Rust everywhere (core, CLI, local server, WASM UI) — no
Node in the product runtime:

| Path | Owns |
|------|------|
| `crates/core` | Platform-neutral rules — cell_id, hex geometry, profile + validation model |
| `crates/cli` | `mapkeeper` binary — filesystem + commands |
| `crates/server` | Local filesystem, world folder, HTTP API, projects list |
| `crates/web` | Rust→WASM UI — calls `core` for logic, `server`/Tauri for filesystem |
| `schemas/` | JSON Schema contracts |
| `fixtures/` | Shared test worlds/profiles |
| `toolchain/` | Author scaffold source of truth (world template) |
| `tests/` | End-to-end tests (Playwright — dev/CI only) |

## License

[Apache License 2.0](LICENSE).

## Status

IDEA + Shape — editor and data contracts ship here as V0 progresses. World projects use the GitHub template above.
