# MAPKEEPER Code Map (Lite)

Short routing index for agents and maintainers.

Use this first, then open only the needed files.

## Task -> first path

- Core rules, ids, geometry, profile model -> `crates/core/src/`
- Spatial contract (distance/ring/range/bounds) -> `crates/core/src/hex.rs`
- Map state model (layers, unknown/none/value, manifest) -> `crates/core/src/layer.rs`
- HTTP API, world file I/O, launcher endpoints -> `crates/server/src/`
- Terrain layer endpoints (`/api/layers/terrain`, `/api/cells/:q/:r/terrain`) -> `crates/server/src/lib.rs`
- CLI commands and query flow (`profile`, `terrain`) -> `crates/cli/src/`
- Web UI (WASM canvas, Home/Editor flow, terrain palette) -> `crates/web/src/`
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
- Project list actions (`open` / `remove` / `delete`, with secondary manage flow): `crates/web/src/lib.rs`, `crates/server/src/lib.rs`
- Default create path suggestion (`Documents/MAPKEEPER Worlds`): `crates/server/src/lib.rs`, `crates/web/src/lib.rs`
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
- Future generators/validators are local tools over these layers (not built,
  not AI runtime).
