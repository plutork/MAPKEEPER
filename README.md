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

Primary path (no installer download):

1. Clone this repository.
2. Open the folder in **Cursor**.
3. First time: run **`.\setup.ps1`**.
4. Run **`.\run.ps1`** (daily — pull when clean, then build + launch).
5. On empty Home, click **Create your first world**.
6. Optional: **`.\update.ps1`** — update-only without launch.
7. If something fails, run **`/doctor`** in Cursor.

Details: [docs/CURSOR-ALPHA.md](docs/CURSOR-ALPHA.md).

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
