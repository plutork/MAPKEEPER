# Development setup

Developer-oriented setup for working on mapkeeper itself.

**Alpha testers:** prefer [CURSOR-ALPHA.md](CURSOR-ALPHA.md) — `.\setup.ps1` then daily `.\run.ps1`; optional `.\update.ps1`; `/doctor` only if stuck.

## Repo layout

Cargo workspace for the active product shell + thin spatial binding:

| Path | Owns |
|------|------|
| `crates/core` | project registry, world identity, immutable `[spatial]` config, spatial foundation + relief field |
| `crates/server` | local HTTP API: health, projects, map-presets, spatial GET/PUT + static UI |
| `crates/web` | Rust → WASM helpers + five-mode shell UI (`index.html` canvas map) |
| `crates/desktop` | Windows desktop shell (Tauri) |
| `archive/map-v2/` | excluded read-only reference; never an active dependency |
| `setup.ps1` / `run.ps1` / `update.ps1` | Windows alpha bootstrap / daily launch with clean-tree pull / explicit update |

## Local server + web UI

```powershell
powershell -File crates/web/build.ps1
cargo run -p mapkeeper-server -- --port 4000 --web-dist crates/web/dist
```

Then open `http://127.0.0.1:4000`.

`mapkeeper-server` without `--world` starts launcher mode. The active API includes
`/api/health`, `/api/projects` (create/open/close/forget/delete), `/api/map-presets`,
`/api/spatial`, and `/api/spatial/field` (relief updates). Archived map-v2 layer,
profile, generator, hydrology, and History APIs are not restored.

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

Runs `cargo test --workspace --exclude mapkeeper-desktop` with
`RUSTFLAGS=-Dwarnings`, clippy, codemap, archive-isolation, encoding, and
doc-drift checks. Optional: `.\scripts\check.ps1 -Smoke` for headless spatial
smoke. If `CODEMAP.md` is stale:

```powershell
python scripts/gen_codemap.py
git add docs/CODEMAP.md
```

Enable repo hook manually: `git config core.hooksPath .githooks`

Requires: Rust (MSVC), `wasm32-unknown-unknown`, WebView2, and Visual Studio Build Tools (C++ workload). Prefer `.\setup.ps1` for first-time checks. If setup fails, use Cursor **`/doctor`** (interactive; no silent MSVC install).

## NSIS installer (Later — not alpha path)

Consumer NSIS packaging is **not** the alpha distribution channel. Do not add installer-first docs/tests/links unless a future decision restores that path.

Maintainer-only local bundle (optional):

```powershell
Set-Location "crates/desktop"
cargo tauri build
```

Output may appear under `target/release/bundle/nsis/`. Signed consumer installer remains Later.
