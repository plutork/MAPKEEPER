# Development setup

Developer-oriented setup for working on mapkeeper itself.

**Alpha testers:** prefer the Cursor agent path in [CURSOR-ALPHA.md](CURSOR-ALPHA.md) (`/mk-doctor` → `/mk-install` → `/mk-run`). This page is for contributors and manual source-run.

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
| `scripts/` | Windows alpha helper scripts (`*-windows.ps1`) |

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

```powershell
powershell -File crates/web/build.ps1
cargo run -p mapkeeper-desktop
```

Or via alpha helpers:

```powershell
powershell -File scripts/doctor-windows.ps1
powershell -File scripts/bootstrap-windows.ps1
powershell -File scripts/run-desktop.ps1
```

Requires: Rust (MSVC), `wasm32-unknown-unknown`, WebView2, and Visual Studio Build Tools (C++ workload). MSVC is a **manual** install — scripts will not silent-install it.

## NSIS installer (Later — not alpha path)

Consumer NSIS packaging is **not** the alpha distribution channel (D-80). Do not add installer-first docs/tests/links unless a future decision restores that path.

Maintainer-only local bundle (optional):

```powershell
Set-Location "crates/desktop"
cargo tauri build
```

Output may appear under `target/release/bundle/nsis/`. Signed consumer installer remains Later.
