# /mk-install — Prepare Cursor workspace (source-run)

**Stance:** prepare this MAPKEEPER checkout for source-run alpha (D-80).

`/mk-install` means **prepare the Cursor workspace**, not install mapkeeper system-wide.

## Do

1. Explain what will be checked/built (Rust toolchain, wasm target, WebView2, web dist, desktop crate).
2. Run from repo root (script asks before heavy steps):

```powershell
powershell -File scripts/bootstrap-windows.ps1
```

3. If MSVC Build Tools are missing: show manual install steps, wait for explicit user confirmation that they finished, then re-check. Never silent-install MSVC.
4. On success, tell the tester to run `/mk-run`.

## Must not

- Silent heavy installs (rustup, MSVC, WebView2, …)
- Patch product source
- Commit / branch
- Delete world folders
- Download or promote NSIS installers
