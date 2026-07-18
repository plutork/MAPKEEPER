# N-026 owner Tauri release gate

Headless Chromium (`scripts/bench-render-scale.mjs`) is **reproducible evidence only**.
**Supported SoT** for Create ceiling honesty is one ordinary Windows alpha-class PC
running **MAPKEEPER Tauri release**.

Do not mark `release_gate.status=passed` in `docs/perf/relief-render-scale-report.json`
unless this checklist was completed on that machine.

## Prep

1. Clean product tree; build release desktop (`.\run.ps1` / release Tauri path per `docs/CURSOR-ALPHA.md`).
2. Note: OS, CPU class, git SHA, build mode = **release**.
3. Create four worlds at catalog footprints ~2k / ~10k / ~25k / ~50k (Create presets).

## Measure (manual or instrumented)

For each size, record p95-ish timings (stopwatch or DevTools Performance is fine):

| Op | Gate? |
|---|---|
| Open / fit | Yes |
| Pan / zoom frames | Yes |
| Stamp drag frames | Yes |
| Airbrush rate 5 frames | Yes |
| Stroke commit (≈64 cells medium stamp) | Yes |
| View Empty full rebuild | Measured, non-gating |
| Relief full rebuild | Measured, non-gating |
| Large stroke commit (≥1200 cells; chunk path if needed) | Measured |
| Memory: process Working Set after open+fit | Required observation |

Budgets: see N-026 in OS decisions (`decisions_post_reset.md`).

## Record

Update report `release_gate`:

```json
"release_gate": {
  "status": "passed" | "failed",
  "owner_run_at": "<ISO-8601>",
  "git_sha": "<sha>",
  "build_mode": "tauri-release",
  "platform": "win32-…",
  "machine_note": "<brief>",
  "memory_working_set_by_size": { "approx_2k": <bytes>, "...": "..." }
}
```

Keep `evidence_class: reproducible_headless` for the Playwright section; do not
relabel headless as final Supported.

## Ceiling

Only after this gate: if 50k fails Supported, apply N-026 ceiling rule via `/idea`
(amend N-016) — never silent catalog drop from headless alone.
