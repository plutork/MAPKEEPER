# mapkeeper

Generic **local** world editor built for the age of AI agents.

The map is not only a picture — it is the interface to a machine-readable world.

## Create your world

**Now (early access):** [GitHub template](https://github.com/plutork/mapkeeper-world-template/generate) — for authors comfortable with git.

**Now (Windows installer, roadmap 5.9):** run the `mapkeeper` desktop app — a native window opens directly on the same Home screen (pick or create a world), no `cargo run`/browser/localhost step. Unsigned for now (Windows SmartScreen will warn — "More info" → "Run anyway"); build it yourself with `cargo tauri build` in `crates/desktop` until packaged releases are published.

**Target UX (long-term):** open mapkeeper → **New world** — pre-configured agents and folders, no git required. The desktop app above is this UX for V0; the editor wizard is the same idea, longer-term.

Details: [toolchain/template/README.md](toolchain/template/README.md).

## Docs

- **[STARTER_PACK.md](STARTER_PACK.md)** — product pitch, invariants, milestones.
- **[docs/CODEMAP-LITE.md](docs/CODEMAP-LITE.md)** — quick task-to-path routing for contributors and agents.

## This repo (product)

Clone MAPKEEPER when you work on **mapkeeper itself** (editor, schemas, contracts) — not when you write lore.

Maintainers and contributors: see source and [toolchain/](toolchain/).

### Repository layout

Cargo workspace, Rust everywhere (core, CLI, local server, WASM UI) — no
Node in the product runtime:

| Path | Owns |
|------|------|
| `crates/core` | Platform-neutral rules — cell_id, hex geometry + spatial contract, profile model, layer-first map-state model (`layer.rs`) |
| `crates/cli` | `mapkeeper` binary — filesystem + commands |
| `crates/server` | Local filesystem, world folder, HTTP API, projects list |
| `crates/web` | Rust→WASM UI — calls `core` for logic, `server`/Tauri for filesystem |
| `crates/desktop` | Windows installer shell (Tauri) — native window over the same `server`+`web`, no browser/localhost step |
| `schemas/` | JSON Schema contracts |
| `fixtures/` | Shared test worlds/profiles |
| `toolchain/` | Author scaffold source of truth (world template) |
| `tests/` | End-to-end tests (Playwright — dev/CI only) |

### Run it locally (dev)

```powershell
powershell -File crates/web/build.ps1     # build the web UI (wasm-bindgen)
cargo run -p mapkeeper-server -- --port 4000 --web-dist crates/web/dist
# open http://127.0.0.1:4000 — Home screen: create a new world (id + folder) or
# open an existing one, then click a hex: Inspect names it (profile), the
# terrain brushes paint the map-state layer. Then query either back:
cargo run -p mapkeeper-cli -- profile get demo.hex.q0.r0 --world .tmp-world
cargo run -p mapkeeper-cli -- terrain get demo.hex.q0.r0 --world .tmp-world
```

`mapkeeper-server` starts in **launcher mode** with no `--world` flag — the
Home screen (backed by `/api/projects`) lists/creates worlds, mirroring the
minimal editor wizard (roadmap 5.7). Pass `--world <path>` to skip the
launcher and open one world directly (handy for scripting/CI). Worlds
created either way are also scaffoldable from the CLI:
`cargo run -p mapkeeper-cli -- init demo --path .tmp-world`.

### Desktop shell (Windows installer)

```powershell
powershell -File crates/web/build.ps1     # same web UI build as above

# quickest — run the native app directly, no installer needed:
cargo run -p mapkeeper-desktop

# or build the real installer:
cd crates/desktop
cargo tauri build
# -> target/release/bundle/nsis/mapkeeper_<version>_x64-setup.exe
```

`cargo tauri build` (unlike plain `cargo build`) has no `--manifest-path` flag — it
resolves `tauri.conf.json` relative to the current directory, so run it from
`crates/desktop`, not the repo root.

`crates/desktop` embeds the exact same `mapkeeper-server` router in-process
(on an OS-assigned port, so it never clashes with a dev server) and opens it
in a native window instead of printing `http://localhost` instructions — the
"New world" folder field also gets a native **Browse…** button there
(`window.__TAURI__` feature-detected in `crates/web`, invisible in a plain
browser tab). Requires the Rust MSVC toolchain + WebView2 (both ship with
modern Windows); `cargo install tauri-cli` once to get the `cargo tauri`
subcommand. Windows only for V0 — code signing and auto-update are Later
(`todo/tauri-after-web-v0.md`).

## License

[Apache License 2.0](LICENSE).

## Status

**V0 done**, plus the **Hex Map Model Foundation**. Full flow works end to
end: open the launcher Home screen (web or desktop app), create or pick a
world, click a hex to name it (profile — `cell_id`/`display_name`/`slug`/
`notes`) or paint terrain, and query either back over the CLI — CI-tested
(schema fixtures + CLI round trip) and dogfooded on a real local world.

The map is a **layer-first world-state model**: machine-readable state lives
under `map/manifest.json` + `map/layers/<id>.json` (sparse per layer — a cell
is `unknown` / `none` / a concrete `value`), kept separate from author
profiles. `terrain` is the first layer; the renderer projects it. Windows
desktop installer (`crates/desktop`, Tauri) wraps the same web UI natively.
Renderer polish, more layers, generators and validators are Later. World
projects also work via the GitHub template above (interim, git-native authors).
