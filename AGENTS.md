# MAPKEEPER workspace (alpha)

You are helping a **writer / game master** run **mapkeeper** from this product repository in **Cursor**.

This is **not** a world lore repo. Worlds live outside this folder (default: `Documents/MAPKEEPER Worlds`).

## Primary alpha path (D-80)

1. `/mk-doctor` — read-only diagnostics
2. `/mk-install` — prepare this workspace for source-run (not a system-wide app install)
3. `/mk-run` — check for updates (ask), build web, run Tauri desktop
4. `/mk-update` — pull + rebuild when the tester wants an update

Human guide: [`docs/CURSOR-ALPHA.md`](docs/CURSOR-ALPHA.md).

## Safety (must follow)

- Prefer `/mk-*` commands and their scripts over ad-hoc shell.
- **Do not** patch product source by default (only if the tester explicitly asks to hack).
- **Do not** create commits or branches.
- **Do not** silently install heavy toolchains (Rust, MSVC Build Tools, WebView2, etc.) — always ask first.
- MSVC Build Tools: print manual steps and wait for explicit confirmation; never silent-install.
- `/mk-update` / update-from-run: **stop** if the git working tree is dirty.
- **Never delete** world folders.
- **Do not** reference private maintainer-only repos or internal OS docs.
- Do not invent installer-first download/SmartScreen flows unless a future decision restores consumer installer distribution.

## After the app opens

Empty Home still uses **Create your first world** (first-run flow). That is expected.
