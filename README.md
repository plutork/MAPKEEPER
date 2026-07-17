# mapkeeper

Local world-workspace shell for writers and game masters.

<a id="product"></a>

## Product

**mapkeeper** currently provides the working desktop shell around portable world
folders. Spatial concept (asymmetric hybrid: world space + hex lattice) is
accepted; base map topology is a bounded planar chart with height (edges do
not wrap). Editor Raise/Lower relief on hexes is the first author-facing
map gesture (`spatial/state.json` + canvas).

The previous hex/layer implementation is suspended and available only as
read-only reference in [`archive/map-v2/`](archive/map-v2/README.md). It is not
compiled, tested, shipped, or used by the active application.

Your lore lives in your world folder, not in this product repository.

### For whom

- **Primary:** writer / GM who opens this repo in **Cursor**, runs the editor from source, and builds worlds in the visual UI (agents help when stuck).
- **Not primary:** standalone consumer-installer onboarding for alpha.

## Alpha (Windows) — workspace-first

Primary path (no installer download). **Full beginner guide:** [docs/CURSOR-ALPHA.md](docs/CURSOR-ALPHA.md).

### First time (never used Cursor or Git)

0. **Windows 10/11**, internet, ~2–5 GB free disk.
1. **Git** — [git-scm.com/download/win](https://git-scm.com/download/win) → install → reopen terminal → `git --version`.
2. **Cursor** — [cursor.com/download](https://cursor.com/download) → install → sign in.
3. **Get MAPKEEPER** (pick one):
   - **Cursor:** **Git: Clone** → `https://github.com/plutork/MAPKEEPER.git` → open folder, or
   - **PowerShell:** `cd $HOME\Documents` → `git clone https://github.com/plutork/MAPKEEPER.git` → in Cursor **File → Open Folder** → that folder.
4. In Cursor terminal (**Ctrl+`**), from repo root: **`.\setup.ps1`** (once; follow prompts; MSVC is manual if asked).
5. **`.\run.ps1`** → desktop app opens → **Create your first world** on empty Home.

### Every day after setup

1. Open the **MAPKEEPER** folder in Cursor.
2. Terminal: **`.\run.ps1`** (pull when git tree is clean, then build + launch).
3. Optional: **`.\update.ps1`** — update-only, no launch.
4. Stuck: **`/doctor`** in Cursor chat.

Details, GitHub Desktop method, safety: [docs/CURSOR-ALPHA.md](docs/CURSOR-ALPHA.md).

## Create your first world

- On empty Home, click **Create your first world**.
- The default root is `Documents/MAPKEEPER Worlds`.
- Creation writes `mapkeeper.toml` (identity + immutable `[spatial]`) and a
  minimal map state. Pick a **map size** card at Create (Default ≈2k cells);
  size is fixed afterward.
- The world opens in the five-mode workspace shell: **Editor, Generator,
  Wizard, Agent, History**.
- Opening a world loads the hex map; Raise/Lower relief with an adjustable
  hard-disk brush (Stamp | Airbrush, Rate 1|5|10|20) in Editor.
  Domain catalogs, generators, and map History are still out of scope.

Your lore stays in the world folder (usually under `Documents/MAPKEEPER Worlds`), not in this product repository.

## Active invariants

<a id="invariants"></a>

- **World ≠ product repository** — author data remains in portable world folders.
- **No AI runtime in product** — agents run outside.
- **Asymmetric hybrid spatial concept** — persisted world space is
  authoritative for continuous position/geometry; hex is the primary discrete
  authoring/simulation lattice and authoritative for grid-bound data; derived
  membership/indexes must not compete as truth. Immutable spatial config lives
  in `mapkeeper.toml` `[spatial]`; mutable content is `spatial/state.json`
  (no screen coordinates). Domain catalogs, map history, and archived map-v2
  contracts are not implied.
- **Local metric world frame** — horizontal world coordinates are meters;
  primary-grid resolution is neighbour-center distance (alpha default 1000 m);
  map size is an extent preset (cols/rows derived). Create offers validated
  wide preset cards (catalog ≤50k cells; Default ≈2k). Silent rescale after
  data exists is forbidden.
- **Base map topology** — bounded planar `(x, y)` chart with single-valued
  height over `(x, y)`; opposite edges do not connect; not a sphere, cylinder,
  or torus. Outside bounds means outside the map.
- **Author relief brush** — first map gesture is Raise/Lower integer relief on
  hex cells with an adjustable hard-disk brush (Stamp default; opt-in Airbrush
  with timed epochs; radius, hover footprint, drag stroke; not soft falloff /
  Flatten / a full terrain editor).
- **Elevation surface SoT** — one grid-bound elevation field; plateaus, ridges,
  and steep neighbour transitions are forms of that field, not separate cliff
  objects. Meshes, shading, and oblique views are display-only.
- **Alpha ocean** — elevation **0** is the coast/land floor; below 0 reads as
  ocean. Relief **Edit ocean** (default off) freezes `h < 0`; on unlocks dig/fill
  ocean; land Lower floors at 0. Not a full water domain or hydrology model.
- **Archived code is reference only** — active code cannot import or copy it
  without a separate architecture decision.
- **Local-only** — no remote telemetry.

## Docs & deeper

- **Cursor alpha guide:** [docs/CURSOR-ALPHA.md](docs/CURSOR-ALPHA.md)
- **Developer setup:** [docs/DEV.md](docs/DEV.md)
- **Code routing map:** [docs/CODEMAP-LITE.md](docs/CODEMAP-LITE.md)
- **map-v2 archive:** [archive/map-v2/README.md](archive/map-v2/README.md)
- **Starter redirect:** [STARTER_PACK.md](STARTER_PACK.md)

## License

[Apache License 2.0](LICENSE).
