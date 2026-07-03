# GitHub world template

**Author onboarding (chosen):** [GitHub Template repository](https://docs.github.com/en/repositories/creating-and-managing-repositories/creating-a-repository-from-a-template).

Authors click **Use this template** — no manual copy from `toolchain/cursor/`.

## Author flow

1. Open **[Create from template](https://github.com/plutork/mapkeeper-world-template/generate)** (repo must exist and be marked *Template repository*).
2. Name your world repo → Create.
3. Clone or open in Cursor.
4. **`/user`** is already installed; folders `map/`, `canon/`, `profiles/`, `data/`, `journal/` are ready.

## Source of truth (maintainers)

Canonical scaffold files live in **`world/`** in this directory.

Publish to the public template repo:

```
MAPKEEPER/toolchain/template/world/  →  github.com/plutork/mapkeeper-world-template  (repo root)
```

After editing `world/` here:

1. Copy contents of `world/` to the `mapkeeper-world-template` repo root.
2. Commit and push.
3. On GitHub: repo **Settings → General → Template repository** ✓

PowerShell example (repos side by side):

```powershell
robocopy "c:\projects\MAPKEEPER\toolchain\template\world" "c:\projects\mapkeeper-world-template" /MIR /XD .git
Set-Location "c:\projects\mapkeeper-world-template"
git add -A
git commit -m "Sync from MAPKEEPER toolchain template."
git push
```

## Legacy (dogfood only)

Manual copy from [../cursor/user.md](../cursor/user.md) — not for end authors. See [../cursor/README.md](../cursor/README.md).

## Later

- Genre templates (`world-lore/`, …) as separate GitHub template repos
- CLI `mapkeeper init` may wrap the same scaffold
