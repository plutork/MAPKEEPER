# Development setup

Developer-oriented setup for working on mapkeeper itself.

**Alpha testers:** prefer [CURSOR-ALPHA.md](CURSOR-ALPHA.md) — `.\setup.ps1` then daily `.\run.ps1`; optional `.\update.ps1`; `/doctor` only if stuck.

## Repo layout

Cargo workspace, Rust everywhere (core, CLI, local server, WASM UI):

| Path | Owns |
|------|------|
| `crates/core` | platform-neutral rules, geometry, profiles, layers |
| `crates/cli` | `mapkeeper` CLI and filesystem commands |
| `crates/server` | local HTTP API + world/project I/O |
| `crates/web` | Rust -> WASM UI |
| `crates/desktop` | Windows desktop shell (Tauri) |
| `schemas/` | JSON Schema contracts |
| `fixtures/` | shared test fixtures |
| `toolchain/` | world scaffold source |
| `tests/` | e2e/dev tooling (Playwright) |
| `setup.ps1` / `run.ps1` / `update.ps1` | Windows alpha bootstrap / daily launch (D-86 pull-in-run) / explicit update |

## Local server + web UI

```powershell
powershell -File crates/web/build.ps1
cargo run -p mapkeeper-server -- --port 4000 --web-dist crates/web/dist
```

Then open `http://127.0.0.1:4000`.

CLI query examples:

```powershell
cargo run -p mapkeeper-cli -- profile get demo.hex.q0.r0 --world .tmp-world
cargo run -p mapkeeper-cli -- terrain get demo.hex.q0.r0 --world .tmp-world
```

`mapkeeper-server` without `--world` starts launcher mode (`/api/projects` list/create/open).

## Desktop shell (Windows) — source-run

First time:

```powershell
.\setup.ps1
.\run.ps1
```

Daily launch (pull when clean, rebuild, launch):

```powershell
.\run.ps1
```

Equivalent manual steps:

```powershell
powershell -File crates/web/build.ps1
cargo run -p mapkeeper-desktop
```

Explicit update on a **clean** tree (rebuild only, no launch):

```powershell
.\update.ps1
```

Before `git push` (or automatically via hook after `.\setup.ps1`):

```powershell
.\scripts\check.ps1
```

Runs `cargo test --workspace --exclude mapkeeper-desktop` with `RUSTFLAGS=-Dwarnings` and codemap drift check (same subset as CI). If `CODEMAP.md` is stale:

```powershell
python scripts/gen_codemap.py
git add docs/CODEMAP.md
```

Enable repo hook manually: `git config core.hooksPath .githooks`

Requires: Rust (MSVC), `wasm32-unknown-unknown`, WebView2, and Visual Studio Build Tools (C++ workload). Prefer `.\setup.ps1` for first-time checks. If setup fails, use Cursor **`/doctor`** (interactive; no silent MSVC install).

## NSIS installer (Later — not alpha path)

Consumer NSIS packaging is **not** the alpha distribution channel (D-80/D-81). Do not add installer-first docs/tests/links unless a future decision restores that path.

Maintainer-only local bundle (optional):

```powershell
Set-Location "crates/desktop"
cargo tauri build
```

Output may appear under `target/release/bundle/nsis/`. Signed consumer installer remains Later.
