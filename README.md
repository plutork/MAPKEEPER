# mapkeeper

Generic **local** world editor built for the age of AI agents.

The map is not only a picture — it is the interface to a machine-readable world.

<a id="product"></a>

## Product

**mapkeeper** helps a **writer / game master** build a portable world folder: hex map, machine-readable layers, and cell profiles that both the editor and agents can query — without an AI runtime inside the app.

Your lore lives in your world folder, not in this product repository.

### For whom

- **Primary:** writer / GM who wants to install the app and start building worlds quickly.
- **Not primary:** source-reading setup flows or manual `.cursor/` bootstrapping.

## Install (Windows alpha 0.2)

1. Open [GitHub Releases](https://github.com/plutork/MAPKEEPER/releases).
2. Download the latest `mapkeeper_*_x64-setup.exe` asset.
3. Install and launch from Start menu / desktop shortcut.

Unsigned alpha builds may show SmartScreen:

- click **More info**
- click **Run anyway**

Tester checklist: [docs/TESTER-NOTES-0.2.0.md](docs/TESTER-NOTES-0.2.0.md).

## Create your first world

- On empty Home, click **Create your first world**.
- Default path is `Documents/MAPKEEPER Worlds`; default size is **Small**.
- Flow opens **Build World wizard** directly.
- Blank **Create** remains available under advanced options.

Git-native interim path stays available via [mapkeeper-world-template](https://github.com/plutork/mapkeeper-world-template/generate).

## Invariants

<a id="invariants"></a>

- **Map -> machine-readable state**, not only a decorative image.
- **Same data** for author UI and agent queries.
- **No AI runtime in product** — agents run outside.
- **Core stays world-agnostic** — private lore remains in world folders.
- **Layer-first map state** (`map/manifest.json` + `map/layers/...`) is separate from human profiles.
- **Local-only** — no remote telemetry in core.

## Docs & deeper

- **Developer setup:** [docs/DEV.md](docs/DEV.md)
- **Code routing map:** [docs/CODEMAP-LITE.md](docs/CODEMAP-LITE.md)
- **World template details:** [toolchain/template/README.md](toolchain/template/README.md)
- **Starter redirect:** [STARTER_PACK.md](STARTER_PACK.md)

## License

[Apache License 2.0](LICENSE).
