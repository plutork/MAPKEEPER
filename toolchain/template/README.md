# World scaffold source

Canonical author-world scaffold: **`world/`** in this directory.

Embedded into the editor/CLI at build time (D-26). Authors create worlds through the **MAPKEEPER editor wizard** (Create your first world), not by generating a separate GitHub template repo.

## Author flow

1. Clone/open [MAPKEEPER](https://github.com/plutork/MAPKEEPER).
2. Alpha Windows: `.\setup.ps1` → `.\run.ps1` (see [CURSOR-ALPHA.md](../../docs/CURSOR-ALPHA.md)).
3. Home → **Create your first world**.
4. Lore lives in the world folder (usually `Documents/MAPKEEPER Worlds`).

## Source of truth

Edit **`world/`** only when changing the scaffold bundle. The running editor is what authors get.

The public GitHub Template sync path (`mapkeeper-world-template`, CI mirror) is **retired** (D-84).

## Legacy (dogfood only)

Manual copy from [../cursor/user.md](../cursor/user.md) — not for end authors.
