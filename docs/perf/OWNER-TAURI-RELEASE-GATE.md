# N-026 / N-039 owner Tauri release gate

Headless Chromium (`scripts/bench-render-scale.mjs` and
`scripts/bench-authoring-performance.mjs`) is **reproducible evidence only**.
**Supported SoT** for Create ceiling honesty is one ordinary Windows alpha-class PC
running **MAPKEEPER Tauri release**.

Do not mark `release_gate.status=passed` in either tracked perf report unless
this checklist was completed on that machine.

## Prep

1. Clean product tree; build the release desktop (`cargo tauri build` from
   `crates/desktop`; see `docs/DEV.md`). Do not use a debug/dev run as SoT.
2. Note: OS, CPU class, git SHA, build mode = **release**.
3. Prepare deterministic ~2k / ~12k / ~26k / ~50k fixtures at relief density
   0 / 25 / 75 / 100%.

## Measure (manual or instrumented)

For each size, record p95-ish timings (stopwatch or DevTools Performance is fine).
After every fresh open, record the **first completed small stroke separately**;
do not include it in steady-state CAR percentiles.

| Op | Gate? |
|---|---|
| Open / fit | Yes |
| Pan / zoom frames | Yes |
| Stamp drag frames | Yes |
| Airbrush rate 5 frames | Yes |
| Stroke commit (≈64 cells medium stamp) | Yes |
| First small stroke after fresh open | Required cold observation; separate from CAR p95 |
| Repeated small + medium `mouseup → ACK → correct frame + next stroke ready` | **Yes: steady p95 ≤100 ms** |
| 100 sequential small strokes on each mature fixture | **Yes: p95 ≤100 ms** |
| View Empty full rebuild | Measured, non-gating |
| Relief full rebuild | Measured, non-gating |
| Large stroke commit (≥1200 cells; chunk path if needed) | Measured |
| Memory: process Working Set after open+fit | Required observation |

Renderer budgets: N-026. Continuous authoring budget and mature matrix: N-039.

## 50k Relief dogfood

On a mature 50k fixture, make one cold stroke after open, then a short ordinary
Relief editing pass before the 100-stroke series. Record whether the author sees
a post-mouseup pause or a rejected/delayed next stroke. Cold-open latency and
steady authoring are two different observations.

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
  "cold_first_stroke_ms_by_fixture": { "approx_50k:100": <ms>, "...": <ms> },
  "steady_car_p95_ms_by_fixture": { "approx_50k:100": <ms>, "...": <ms> },
  "relief_dogfood_note": "<visible pause / next-stroke readiness>",
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
that storage change or start it automatically. Named places remain deferred
until this gate and dogfood choose the next step.
