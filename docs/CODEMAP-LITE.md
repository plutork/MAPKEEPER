# MAPKEEPER Code Map (Lite)

Short routing index for agents and maintainers.

Use this first, then open only the needed files.

## Task -> first path

- Core rules, ids, geometry, profile model -> `crates/core/src/`
- Spatial contract (distance/ring/range/bounds) -> `crates/core/src/hex.rs`
- Map size presets (Small/Medium/Large -> hex-radius) -> `crates/core/src/map_preset.rs`
- Map state model (layers, unknown/none/value, manifest) -> `crates/core/src/layer.rs`
- HTTP API, world file I/O, launcher endpoints -> `crates/server/src/`
- Terrain layer endpoints (`/api/layers/terrain`, `/api/cells/:q/:r/terrain`) -> `crates/server/src/lib.rs`
- CLI commands and query flow (`profile`, `terrain`) -> `crates/cli/src/`
- Web UI (WASM canvas, Home/Editor flow, tool dock + terrain brushes) -> `crates/web/src/`
- Desktop shell (Tauri wrapper, native dialog bridge) -> `crates/desktop/src/`
- Data contracts and fixtures -> `schemas/`, `fixtures/`
- World scaffold source -> `toolchain/template/world/`
- CI/build behavior -> `.github/workflows/`

## Key files

- Workspace members: `Cargo.toml`
- Core boundary entry: `crates/core/src/lib.rs`
- Server boundary entry: `crates/server/src/lib.rs`
- Web boundary entry: `crates/web/src/lib.rs`
- Home screen layout entry: `crates/web/index.html`
- Editor tool dock (rail + collapsible drawers: Inspect/profile, Terrain brushes, View stub, World): `crates/web/index.html`, `crates/web/src/lib.rs` — **overlays** the map (D-39); canvas stable on drawer toggle
- Project list actions (`open` / `remove` / `delete`, with secondary manage flow): `crates/web/src/lib.rs`, `crates/server/src/lib.rs`
- Default create path suggestion (`Documents/MAPKEEPER Worlds`): `crates/server/src/lib.rs`, `crates/web/src/lib.rs`
- Map bounds at create (`map_preset`, `write_map_manifest`, `/api/map` bounds + `legacy_map`): `crates/server/src/lib.rs`, `crates/core/src/map_preset.rs`
- Home Create/Generate preset selectors + bounds-driven redraw: `crates/web/index.html`, `crates/web/src/lib.rs`
- CLI `init --map-preset`: `crates/cli/src/main.rs`
- Desktop boundary entry: `crates/desktop/src/lib.rs`
- CLI boundary entry: `crates/cli/src/main.rs`

## Boundary rule

- New procedural/generative map logic goes to `crates/core`.
- `web`, `server`, `desktop`, `cli` call into core and own adapter concerns only.

## Model boundary (D-36)

- **Map state = layers** under `map/layers/<id>.json` (machine-readable, sparse
  `cell_id -> {state}`; missing key = `unknown`). Model in `core::layer`.
- **Author profiles** (`profiles/*.json`) are human-facing and **not** a layer.
- Renderer is a projection of the layer model, not the source of truth.
- Renderer layout (4.2): fit-to-window canvas — `hex_layout`/`map_half_extent`
  compute hex size + origin from the live canvas box (`sync_canvas_size` +
  window `resize`); `unknown`/`none`/`value` fills are visually distinct
  (`none` gets an × marker). All in `crates/web/src/lib.rs`.
- Future generators/validators are local tools over these layers (not built,
  not AI runtime).
