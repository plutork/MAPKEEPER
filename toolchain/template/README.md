# GitHub world template

**Author onboarding (interim):** [GitHub Template repository](https://docs.github.com/en/repositories/creating-and-managing-repositories/creating-a-repository-from-a-template).

Authors click **Use this template** — no manual copy from `toolchain/cursor/`.

Long-term primary UX for writers: **editor wizard «New world»** (same scaffold bundle).

## Author flow

1. Open **[Create from template](https://github.com/plutork/mapkeeper-world-template/generate)** (repo must exist and be marked *Template repository*).
2. Name your world repo → Create.
3. Clone or open in Cursor.
4. **`/user`** is already installed; folders `map/`, `canon/`, `profiles/`, `data/`, `journal/` are ready.

## Source of truth

Canonical scaffold: **`world/`** in this directory.

Published to: [github.com/plutork/mapkeeper-world-template](https://github.com/plutork/mapkeeper-world-template) (repo root).

## Sync (automated — do not hand-copy)

### CI (default)

On push to `main` when `toolchain/template/world/**` changes, GitHub Actions runs [`.github/workflows/sync-world-template.yml`](../../.github/workflows/sync-world-template.yml) and pushes to `mapkeeper-world-template`.

**One-time setup** (maintainer):

1. Create public repo `plutork/mapkeeper-world-template`; enable **Template repository** in Settings.
2. In **MAPKEEPER** repo → Settings → Secrets → Actions → add `MAPKEEPER_WORLD_TEMPLATE_PAT`  
   Fine-grained PAT: **Contents: Read and write** on `mapkeeper-world-template` only.
3. Push scaffold once (or run workflow manually: Actions → Sync world template → Run workflow).

Agents edit **`world/` only** — CI publishes the template repo.

### Local script (optional)

```powershell
Set-Location "c:\projects\MAPKEEPER"
.\toolchain\template\sync-template.ps1 -Push
```

Sibling clone default: `..\mapkeeper-world-template`. Override with `-TargetRepoPath`. Use `-DryRun` to preview.

## Legacy (dogfood only)

Manual copy from [../cursor/user.md](../cursor/user.md) — not for end authors.

## Later

- Same `world/` bundle packaged in editor wizard (V0)
- Genre templates as separate GitHub template repos
- CLI `mapkeeper init` may wrap the same scaffold
