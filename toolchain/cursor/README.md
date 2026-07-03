# Cursor author commands

Install in your **world workspace** — not in the mapkeeper product repo.

## Install `/user`

1. In your world repo, create `.cursor/commands/` if it does not exist.
2. Copy the full contents of [user.md](user.md) into `your-world/.cursor/commands/user.md`.
3. Open **your-world** as the Cursor workspace root (File → Open Folder).
4. Confirm `/user` appears in the command palette.

## What it does

The `/user` command makes the agent behave as an **author** using mapkeeper: query profiles, edit canon, report friction — without patching editor source.

## Verify

- Agent reads world data and mapkeeper contracts, not implementation code.
- Gaps → friction notes and optional [GitHub issues](https://github.com/plutork/MAPKEEPER/issues).

## Optional context in world repo

Add `AGENTS.md` in your world root, or link to [STARTER_PACK.md](https://github.com/plutork/MAPKEEPER/blob/main/STARTER_PACK.md) (product pitch).

Parent index: [../README.md](../README.md).
