# Development setup

Developer-oriented setup for working on mapkeeper itself (not world author onboarding).

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

## Desktop shell (Windows)

```powershell
powershell -File crates/web/build.ps1
cargo run -p mapkeeper-desktop
```

Build NSIS installer:

```powershell
Set-Location "crates/desktop"
cargo tauri build
```

Output:

`target/release/bundle/nsis/mapkeeper_<version>_x64-setup.exe`

Notes:

- run `cargo tauri build` from `crates/desktop` (config path resolution);
- requires Rust MSVC toolchain + WebView2;
- install Tauri CLI once: `cargo install tauri-cli`.
