# Cursor alpha (Windows)

Workspace-first alpha for mapkeeper (D-80 direction, **D-81** surface).

## Path

1. Clone this repository.
2. Open the folder in **Cursor**.
3. Run **`.\run.ps1`** in a terminal (builds web + launches the desktop app).
4. On empty Home, use **Create your first world**.
5. Later: **`.\update.ps1`**.
6. If something fails: run **`/doctor`** in Cursor (agent diagnoses and repairs with your consent).

No installer download. No agent required for normal run/update.

## What this validates

Open the MAPKEEPER workspace → run the visual editor from source → author creates/resumes a world → agents can use the same world data.

## Safety

- `run.ps1` does not update git or install toolchains silently.
- `update.ps1` stops on a dirty tree; uses `git pull --ff-only` only.
- `/doctor` asks before heavy installs; never deletes world folders.

## Not this alpha

- NSIS / SmartScreen installer-first distribution
- Portable runtime zip
- In-app Check for updates
- `/mk-*` commands or `doctor.ps1`
- macOS / Linux root scripts

Contributor details: [`DEV.md`](DEV.md).
