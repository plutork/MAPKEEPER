# Cursor alpha (Windows)

Workspace-first alpha for mapkeeper (D-80 direction, **D-83** surface).

## Path

1. Clone this repository.
2. Open the folder in **Cursor**.
3. First time: run **`.\setup.ps1`** (asks before installs; prepares this workspace).
4. Run **`.\run.ps1`** (builds web + launches the desktop app).
5. On empty Home, use **Create your first world**.
6. Later: **`.\update.ps1`**.
7. If something fails: run **`/doctor`** in Cursor (agent diagnoses and repairs with your consent).

No installer download. No agent required for setup/run/update happy path.

## What this validates

Open the MAPKEEPER workspace → prepare/run the visual editor from source → author creates/resumes a world → agents can use the same world data.

## Safety

- `setup.ps1` asks before heavy changes; never silent-installs MSVC; does not git pull.
- `run.ps1` does not update git or install toolchains.
- `update.ps1` stops on a dirty tree; uses `git pull --ff-only` only.
- `/doctor` asks before heavy installs; never deletes world folders.

## Not this alpha

- NSIS / SmartScreen installer-first distribution
- Portable runtime zip
- In-app Check for updates
- `/mk-*` commands or `doctor.ps1`
- macOS / Linux root scripts

Contributor details: [`DEV.md`](DEV.md).
