# /mk-run — Run Tauri desktop from source

**Stance:** launch the visual editor via source-run (D-80).

## Do

1. Run from repo root:

```powershell
powershell -File scripts/run-desktop.ps1
```

2. Script behavior (do not bypass):
   - lightweight update check;
   - if updates available and git is clean → **ask** before pull/rebuild;
   - if git is dirty → warn, do not auto-update, offer launch current;
   - build web (`crates/web/build.ps1`);
   - `cargo run -p mapkeeper-desktop`.
3. After the window opens, remind: empty Home → **Create your first world**.

## Must not

- Silent update / force-clean
- Patch product source
- Commit / branch
- Use browser localhost as the primary alpha path
- Delete world folders
