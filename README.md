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

### Run it locally (dev)

```powershell
powershell -File crates/web/build.ps1     # build the web UI (wasm-bindgen)
cargo run -p mapkeeper-server -- --port 4000 --web-dist crates/web/dist
# open http://127.0.0.1:4000 — Home screen: create a new world (id + folder) or
# open an existing one, then click a hex, save a title/notes. Then:
cargo run -p mapkeeper-cli -- profile get demo.hex.q0.r0 --world .tmp-world
```

`mapkeeper-server` starts in **launcher mode** with no `--world` flag — the
Home screen (backed by `/api/projects`) lists/creates worlds, mirroring the
minimal editor wizard (roadmap 5.7). Pass `--world <path>` to skip the
launcher and open one world directly (handy for scripting/CI). Worlds
created either way are also scaffoldable from the CLI:
`cargo run -p mapkeeper-cli -- init demo --path .tmp-world`.

## License

[Apache License 2.0](LICENSE).

## Status

V0 in progress. Flow-first slice works end to end: open the launcher Home
screen, create or pick a world, paint a hex cell in the local web UI, save a
profile (real V0 fields — `cell_id`/`display_name`/`slug`/`notes`), query it
back over the CLI. Renderer polish and validation strictness are still open.
World projects also work via the GitHub template above (interim, git-native
authors).
