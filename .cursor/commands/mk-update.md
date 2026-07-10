# /mk-update — Update product + rebuild

**Stance:** update this checkout and rebuild for source-run alpha (D-80).

## Do

1. Run from repo root:

```powershell
powershell -File scripts/update-windows.ps1
```

2. Script behavior:
   - if git dirty → **stop** and show `git status`;
   - else `git pull --ff-only`;
   - rebuild web + confirm desktop crate builds;
   - report old → new result.
3. Suggest `/mk-run` when done.

## Must not

- Force-clean or discard local changes
- Silent toolchain installs
- Commit / branch (beyond the pull itself)
- Delete world folders
- Patch product source
