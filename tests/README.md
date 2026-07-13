# Tests

End-to-end tests (Playwright) for the local web V0 author story: wizard →
paint hex cell → save profile → CLI query.

Node is a **dev/CI-only** dependency here, not part of the product runtime
(see D-20 stack decision). See the `mapkeeper-web-tests` skill.

## Headless API smoke (CI / maintainer agents)

`scripts/smoke-headless.ps1` — no browser, no `Read-Host`:

- Copies `fixtures/worlds/gentle-plain` to a temp folder
- Starts `mapkeeper-server` with `--world`
- Asserts `GET /api/map`, `/api/integrity`, `/api/layers/elevation`
- Stops the server and deletes the temp world (even on failure)

```powershell
.\scripts\smoke-headless.ps1
# optional: .\scripts\check.ps1 -Smoke
```

CI runs this job on Linux via `pwsh`. Env: `SMOKE_PORT`, `MAPKEEPER_SMOKE_SKIP_WEB_BUILD=1` if `crates/web/dist` already exists.

## Manual Playwright smoke (D-21 flow-first + launcher)

`smoke.mjs` is a one-off Playwright script proving the launcher → hex map →
placeholder-profile flow end to end. It is **not** wired into default `check.ps1`
or `run.ps1` — run explicitly when debugging UI.

`chromium.launch({ headless })` defaults to **headless** unless `SMOKE_HEADED=1`.

```powershell
npm install                                  # in this folder, once
powershell -File ..\crates\web\build.ps1     # build the WASM UI -> crates/web/dist
cargo run -p mapkeeper-server -- --port 4100 --web-dist ..\crates\web\dist
# in another shell (creates a world named smoke-world via the web wizard):
npm run smoke -- http://127.0.0.1:4100 smoke-world C:\projects\smoke-world
cargo run -p mapkeeper-cli -- profile get smoke-world.hex.q0.r0 --world C:\projects\smoke-world
```

Passing `--world <path>` to `mapkeeper-server` skips the launcher Home
screen (server opens straight into the editor); `smoke.mjs` detects this and
skips the wizard step.

## Non-interactive setup

```powershell
.\setup.ps1 -NonInteractive          # no Read-Host; fails if toolchain missing
.\setup.ps1 -NonInteractive -InstallToolchain   # explicit consent for rustup/winget/cargo install
$env:MAPKEEPER_SETUP_YES = "1"       # same as -Yes / -NonInteractive
```
