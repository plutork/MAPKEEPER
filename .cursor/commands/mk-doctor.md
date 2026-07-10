# /mk-doctor — Diagnose alpha workspace

**Stance:** read-only diagnostics for agent-managed alpha (D-80).

## Do

1. Run from repo root:

```powershell
powershell -File scripts/doctor-windows.ps1
```

2. Summarize OK / missing items in plain language.
3. Suggest next step: `/mk-install` if not ready, else `/mk-run`.

## Must not

- Install or download anything
- Patch product source
- Commit / branch
- Touch world folders
