# Tests

End-to-end tests (Playwright) for the local web V0 author story: wizard →
paint hex cell → save profile → CLI query.

Node is a **dev/CI-only** dependency here, not part of the product runtime
(see D-20 stack decision). See the `mapkeeper-web-tests` skill.

## Manual smoke (D-21 flow-first + launcher)

`smoke.mjs` is a one-off Playwright script proving the launcher → hex map →
placeholder-profile flow end to end (real Playwright *suite* is still open —
see the `mapkeeper-web-tests` skill for the eventual test structure).

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
