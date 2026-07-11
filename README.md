# mapkeeper

Generic **local** world editor built for the age of AI agents.

The map is not only a picture — it is the interface to a machine-readable world.

<a id="product"></a>

## Product

**mapkeeper** helps a **writer / game master** build a portable world folder: hex map, machine-readable layers, and cell profiles that both the editor and agents can query — without an AI runtime inside the app.

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
- Default path is `Documents/MAPKEEPER Worlds`; default size is **Small**.
- Flow opens **Build World wizard** directly.
- Blank **Create** remains available under advanced options.

Your lore stays in the world folder (usually under `Documents/MAPKEEPER Worlds`), not in this product repository.

## Invariants

<a id="invariants"></a>

- **Map -> machine-readable state**, not only a decorative image.
- **Same data** for author UI and agent queries.
- **No AI runtime in product** — agents run outside.
- **Core stays world-agnostic** — private lore remains in world folders.
- **Layer-first map state** (`map/manifest.json` + `map/layers/...`) is separate from human profiles.
- **Local-only** — no remote telemetry in core.

## Docs & deeper

- **Cursor alpha guide:** [docs/CURSOR-ALPHA.md](docs/CURSOR-ALPHA.md)
- **Developer setup:** [docs/DEV.md](docs/DEV.md)
- **Code routing map:** [docs/CODEMAP-LITE.md](docs/CODEMAP-LITE.md)
- **World template details:** [toolchain/template/README.md](toolchain/template/README.md)
- **Starter redirect:** [STARTER_PACK.md](STARTER_PACK.md)

## License

[Apache License 2.0](LICENSE).
