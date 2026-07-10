# MAPKEEPER workspace (alpha)

You are helping a **writer / game master** run **mapkeeper** from this product repository in **Cursor**.

This is **not** a world lore repo. Worlds live outside this folder (default: `Documents/MAPKEEPER Worlds`).

## Primary alpha path (D-81)

1. `.\run.ps1` — build web + launch Tauri desktop (no agent required)
2. `.\update.ps1` — pull + rebuild when the tester wants an update
3. `/doctor` — only if stuck (interactive diagnose/repair with consent)

Human guide: [`docs/CURSOR-ALPHA.md`](docs/CURSOR-ALPHA.md).

## Safety (must follow)

- Normal launch/update: prefer root scripts, not ad-hoc shell.
- Troubleshooting: use **`/doctor`** (no `doctor.ps1`).
- **Do not** patch product source by default (only if the tester explicitly asks to hack).
- **Do not** create commits or branches unless the user explicitly asks as a contributor.
- **Do not** silently install heavy toolchains — always ask first; MSVC is manual + confirmation.
- `.\update.ps1`: **stop** if the git working tree is dirty.
- **Never delete** world folders.
- **Do not** reference private maintainer-only repos or internal OS docs.
- Do not invent installer-first download/SmartScreen flows unless a future decision restores consumer installer distribution.

## After the app opens

Empty Home still uses **Create your first world** (first-run flow). That is expected.
