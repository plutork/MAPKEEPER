---
name: mapkeeper-web-tests
description: >-
  Playwright tests for mapkeeper local web V0 — wizard, hex map, profile query
  smoke. Use when writing e2e tests, debugging UI, or CI browser checks.
paths: tests/**, e2e/**, **/playwright/**
---

# Web tests (mapkeeper V0)

**Adapted from:** [anthropics/skills webapp-testing](https://github.com/anthropics/skills/tree/main/skills/webapp-testing) — trimmed; no bundled scripts required.

## V0 acceptance smoke *(D-12)*

Cover author story at minimum:

1. Wizard creates world folder + scaffold.
2. Paint at least one hex cell.
3. Save/edit cell profile JSON.
4. CLI profile query returns expected cell (or file fallback).

## Playwright pattern

```python
from playwright.sync_api import sync_playwright

with sync_playwright() as p:
    browser = p.chromium.launch(headless=True)
    page = browser.new_page()
    page.goto("http://localhost:<port>")
    page.wait_for_load_state("networkidle")
    # ... actions from rendered DOM
    browser.close()
```

## Rules

- **Reconnaissance first** on dynamic SPA: screenshot → pick selectors → act.
- Wait `networkidle` before DOM inspection.
- Prefer `role=` and `text=` selectors over brittle CSS.
- Tests live in repo — **no** maintainer lore content in fixtures.

## CI

- **Default CI smoke:** `scripts/smoke-headless.ps1` (API only, temp fixture world) — see `tests/README.md`.
- **Optional UI smoke:** `tests/smoke.mjs` — `chromium.launch({ headless: true })` unless `SMOKE_HEADED=1`.
- Schema/query unit tests separate from Playwright — both useful for V0-done (D-12).

## Gate

Add tests under **`/real`** once web app exists; scope agreed in D-12.
