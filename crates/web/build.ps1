# Build the shell-only WASM UI and stage static assets.
$ErrorActionPreference = "Stop"
$env:RUSTFLAGS = "-Dwarnings"
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
# HTML is source-of-truth -- copy after wasm-bindgen (it may emit a stub index).
$indexSrc = Join-Path $PSScriptRoot "index.html"
$indexDst = Join-Path $dist "index.html"
Copy-Item $indexSrc $indexDst -Force
$staged = Get-Content $indexDst -Raw
if ($staged -notmatch 'id="home"' -or $staged -notmatch "mapkeeper_web") {
    throw "dist/index.html staging failed; refusing stale UI"
}
Write-Host "mapkeeper shell built -> $dist"
