# /doctor — Interactive alpha troubleshooting

**Stance:** agent-native diagnose + repair with consent (D-81). Amends D-80 surface.

There is **no** `doctor.ps1`. Run checks yourself in the shell; keep the conversation with the tester.

## When

Tester ran `.\run.ps1` or `.\update.ps1` and it failed, or the environment looks broken.

## Do

1. Run environment checks directly (read-only first): Git, Rust/`rustc`, Cargo, MSVC (`cl`), WebView2, `wasm32-unknown-unknown`, `wasm-bindgen`, `crates/web/dist`, ability to build web / run desktop.
2. Diagnose the failure class in plain language.
3. Ask clarifying questions only when needed.
4. Propose a **fix plan** before executing.
5. Ask **explicit confirmation** before heavy installs or PATH changes (rustup, MSVC Build Tools manual steps, WebView2, `wasm-bindgen-cli`, rustup targets, etc.). Never silent-install MSVC.
6. Execute only safe consented fixes; re-run checks after.
7. When ready, tell the tester to run `.\run.ps1` (or `.\update.ps1` if that was the goal).

## Must not

- Patch product source by default
- Commit / branch unless the user explicitly asks as a contributor
- Silent heavy installs
- Delete world folders
- Reference private maintainer-only repos
- Invent installer-first / SmartScreen download flows
- Replace normal launch/update — those stay `.\run.ps1` / `.\update.ps1`
