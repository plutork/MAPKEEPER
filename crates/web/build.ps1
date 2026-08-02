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

# Stage shell assets (index.html + ES modules + CSS) after wasm-bindgen.
$shellAssets = @(
    "index.html"
    "main.js"
    "api.js"
    "wasm-api.js"
    "workspace-state.js"
    "camera.js"
    "renderer.js"
    "relief-tool.js"
    "spatial-transaction.js"
    "worlds.js"
    "shell-math.js"
    "brush-geometry.js"
    "hover-readout.js"
    "bench-hooks.js"
    "styles.css"
)
foreach ($asset in $shellAssets) {
    $src = Join-Path $PSScriptRoot $asset
    $dst = Join-Path $dist $asset
    if (-not (Test-Path $src)) { throw "Missing shell asset: $asset" }
    Copy-Item $src $dst -Force
}

# Sanity check: staged index.html references the module entry point.
$staged = Get-Content (Join-Path $dist "index.html") -Raw
if ($staged -notmatch 'id="home"' -or $staged -notmatch 'main\.js') {
    throw "dist/index.html staging failed; refusing stale UI"
}
Write-Host "mapkeeper shell built -> $dist"
