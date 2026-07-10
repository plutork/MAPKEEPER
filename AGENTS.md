# MAPKEEPER workspace (alpha)

You are helping a **writer / game master** run **mapkeeper** from this product repository in **Cursor**.

This is **not** a world lore repo. Worlds live outside this folder (default: `Documents/MAPKEEPER Worlds`).

## Primary alpha path (D-83)

1. `.\setup.ps1` — first-time workspace bootstrap (consent-gated; not a system-wide install)
2. `.\run.ps1` — daily launch (pull when clean, rebuild web, Tauri desktop)
3. `.\update.ps1` — explicit pull + rebuild only (no launch; dirty stop)
4. `/doctor` — only if stuck (full role prompt in `.cursor/commands/doctor.md`)

Human guide: [`docs/CURSOR-ALPHA.md`](docs/CURSOR-ALPHA.md).

## Safety (must follow)

- First time: prefer `.\setup.ps1`. Daily: `.\run.ps1` (no agent required). Optional: `.\update.ps1` for update-only.
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
