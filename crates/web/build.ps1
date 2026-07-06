# Builds mapkeeper-web to WASM and stages it as static files for mapkeeper-server.
# No trunk / wasm-pack dependency — plain cargo + wasm-bindgen-cli (must match
# the `wasm-bindgen` crate version pinned in Cargo.toml).
#
# Usage (from repo root or this folder): powershell -File crates/web/build.ps1

$ErrorActionPreference = "Stop"
# repo root is two levels up: crates/web -> crates -> root (workspace Cargo.toml lives there)
$root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$dist = Join-Path $PSScriptRoot "dist"

Push-Location $root
try {
    cargo build -p mapkeeper-web --target wasm32-unknown-unknown --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
}
finally {
    Pop-Location
}

New-Item -ItemType Directory -Force -Path $dist | Out-Null

$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $root "target" }
$wasmPath = Join-Path $targetDir "wasm32-unknown-unknown/release/mapkeeper_web.wasm"
wasm-bindgen $wasmPath --target web --no-typescript --out-dir $dist
if ($LASTEXITCODE -ne 0) { throw "wasm-bindgen failed" }

Copy-Item (Join-Path $PSScriptRoot "index.html") $dist -Force

Write-Host "mapkeeper-web built -> $dist"
