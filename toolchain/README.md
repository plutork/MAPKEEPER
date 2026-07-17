# Author toolchain

## Create your world (recommended)

1. Open **MAPKEEPER** in Cursor (alpha: `.\setup.ps1` → `.\run.ps1`).
2. On Home, click **Create your first world**.
3. Work in your world folder (usually `Documents/MAPKEEPER Worlds`) — not in this product repo.

Details: [docs/CURSOR-ALPHA.md](../docs/CURSOR-ALPHA.md).

## Layout in this repo

| Path | Purpose |
|------|---------|
| [template/](template/) | Reset note for the retired map-v2 scaffold |
| `../archive/map-v2/toolchain/` | Read-only old scaffold and agent command reference |

During the reset, the application creates only `mapkeeper.toml`. No map,
profiles, canon, data, journal, or `.cursor` contract is scaffolded.

Maintainer tooling lives outside this public repo.
