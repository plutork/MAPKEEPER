# Cursor alpha (Windows)

Agent-managed alpha for mapkeeper (D-80).

## Path

1. Clone this repository.
2. Open the folder in **Cursor**.
3. Run **`/mk-doctor`**.
4. Run **`/mk-install`** if needed (prepares this workspace — not a system-wide install).
5. Run **`/mk-run`** (update check → build web → Tauri desktop).
6. On empty Home, use **Create your first world**.
7. Later: **`/mk-update`**.

## What this validates

Cursor opens the MAPKEEPER workspace → agent prepares/runs the visual editor → author creates/resumes a world → agents can use the same world data.

## Safety

- No silent heavy installs; MSVC Build Tools need a manual step + confirmation.
- Alpha agent does not patch product source by default.
- No commits/branches by the alpha agent.
- Update stops on a dirty git tree (`git pull --ff-only` only when clean).
- World folders are never deleted by these commands.

## Not this alpha

- NSIS / SmartScreen installer-first distribution
- Portable runtime zip
- In-app Check for updates
- macOS / Linux scripts

Contributor details: [`DEV.md`](DEV.md).
