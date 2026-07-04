# Tests

End-to-end tests (Playwright) for the local web V0 author story: wizard →
paint hex cell → save profile → CLI query.

Node is a **dev/CI-only** dependency here, not part of the product runtime
(see MAPKEEPER-OS `decisions.md` D-20). See the `mapkeeper-web-tests` skill.

## Manual smoke (D-21 flow-first)

`smoke.mjs` is a one-off Playwright script proving the placeholder-profile
flow end to end (real Playwright *suite* is still open — see the
`mapkeeper-web-tests` skill for the eventual test structure).

```powershell
npm install                                  # in this folder, once
powershell -File ..\crates\web\build.ps1     # build the WASM UI -> crates/web/dist
cargo run -p mapkeeper-cli -- init demo --path .tmp-world
cargo run -p mapkeeper-server -- --world .tmp-world --port 4100 --web-dist ..\crates\web\dist
# in another shell:
npm run smoke -- http://127.0.0.1:4100
cargo run -p mapkeeper-cli -- profile get demo.hex.q0.r0 --world .tmp-world
```
