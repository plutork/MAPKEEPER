# N-026 / N-039 owner Tauri release gate

Headless Chromium (`scripts/bench-render-scale.mjs` and
`scripts/bench-authoring-performance.mjs`) is **reproducible evidence only**.
**Supported SoT** for Create ceiling honesty is one ordinary Windows alpha-class PC
running **MAPKEEPER Tauri release**.

Do not mark `release_gate.status=passed` in either tracked perf report unless
this checklist was completed on that machine.

## Prep

1. Clean product tree; build release desktop (`.\run.ps1` / release Tauri path per `docs/CURSOR-ALPHA.md`).
2. Note: OS, CPU class, git SHA, build mode = **release**.
3. Prepare deterministic ~2k / ~12k / ~26k / ~50k fixtures at relief density
   0 / 25 / 75 / 100%.

## Measure (manual or instrumented)

For each size, record p95-ish timings (stopwatch or DevTools Performance is fine):

| Op | Gate? |
|---|---|
| Open / fit | Yes |
| Pan / zoom frames | Yes |
| Stamp drag frames | Yes |
| Airbrush rate 5 frames | Yes |
| Stroke commit (≈64 cells medium stamp) | Yes |
| `mouseup → durable ACK → first correct frame + next stroke ready` | **Yes: p95 ≤100 ms** |
| 100 sequential small strokes on each mature fixture | **Yes: p95 ≤100 ms** |
| View Empty full rebuild | Measured, non-gating |
| Relief full rebuild | Measured, non-gating |
| Large stroke commit (≥1200 cells; chunk path if needed) | Measured |
| Memory: process Working Set after open+fit | Required observation |

Renderer budgets: N-026. Continuous authoring budget and mature matrix: N-039.

## Record

Update `release_gate` in both
`relief-render-scale-report.json` and `large-map-authoring-report.json`:

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

Only after this gate: if 50k fails Supported, apply N-026 ceiling rule via
`/idea` (amend N-016) — never silently drop catalog rungs from headless alone.
If CAR fails because full-file persistence dominates, follow N-039 with a
separate `/idea` for dense versioned relief storage; this gate does not approve
that storage change.
