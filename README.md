# mapkeeper

Generic **local** world editor built for the age of AI agents.

The map is not only a picture — it is the interface to a machine-readable world.

<a id="product"></a>

## Product

**mapkeeper** helps a **writer / game master** build a portable world folder: hex map, machine-readable layers, and cell profiles that the same tools (and Cursor agents) can query — without an AI runtime inside the app.

Your **lore lives in a world folder**, not in this product repository. Clone or install mapkeeper to *run the editor*; create worlds under Documents (or any folder you choose).

### For whom

- **Primary:** writer / GM — open the app, build or resume a world, agents already understand the layout.
- **Not primary:** reading mapkeeper source, hand-copying `.cursor/` files, or treating GitHub as the only onboarding path.

### Invariants

<a id="invariants"></a>

- **Map → machine-readable state**, not only a decorative image.
- **Same data** for the author (visual editor) and for agents (profiles + layer contracts).
- **No AI runtime in the product** — agents run outside; mapkeeper ships data contracts and world scaffolds.
- **Core stays world-agnostic** — private lore stays in the world folder.
- **Layer-first map state** (`map/manifest.json` + `map/layers/…`) is separate from human **profiles** (both keyed by cell id).
- **Local-only** — no remote telemetry in core.
- **Onboarding preference:** desktop / in-app Build World → GitHub world template (interim) → CLI `init` (power users).

### Where we are

| Horizon | Intent |
|---------|--------|
| **Now** | V0 editor path shipped: Home launcher, desktop shell (Windows), Build World wizard (size → silhouette → tectonics → elevation), hex map + profiles + CLI query, layer-first terrain/geology/elevation |
| **Next** | Climate / rivers and further Geo pipeline steps; polish from dogfood |
| **Later** | Canon UI, time slices, more layers/generators/validators, signed installers / other OS |

---

## Create your world

**Now (Windows desktop):** install from [GitHub Releases](https://github.com/plutork/MAPKEEPER/releases) (alpha pre-releases) or run `mapkeeper` locally. Home screen handles pick/create flow directly (no browser/`localhost` step). Unsigned alpha builds may show SmartScreen — "More info" → "Run anyway". Tester checklist: [docs/TESTER-NOTES-0.2.0.md](docs/TESTER-NOTES-0.2.0.md).

**Now (git-native):** [GitHub world template](https://github.com/plutork/mapkeeper-world-template/generate).

**Target UX (long-term):** open mapkeeper → **New world** with agents and folders ready, no git required. Desktop + Build World are that path for V0; polish continues.

Details: [toolchain/template/README.md](toolchain/template/README.md).

## Docs

- **This README** — product pitch, invariants, and how to run (canonical; formerly `STARTER_PACK.md`).
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
