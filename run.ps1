# alpha-root-scripts-simplify (D-81/D-83): launch only — no update, no setup installs
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
Set-Location $Root

function Fail-Preflight($Msg) {
    Write-Host $Msg
    Write-Host "First time? run .\setup.ps1"
    Write-Host "Still stuck? run /doctor in Cursor."
    exit 1
}

Write-Host "mapkeeper run (Windows source-run)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    Fail-Preflight "Not in MAPKEEPER repo root (Cargo.toml missing)."
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail-Preflight "cargo not found."
}

if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Fail-Preflight "rustc not found."
}

Write-Host "Building web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Web build failed."
    Write-Host "First time? run .\setup.ps1"
    Write-Host "Still stuck? run /doctor in Cursor."
    exit $LASTEXITCODE
}

Write-Host "Launching mapkeeper-desktop…"
Write-Host "After Home opens: use Create your first world if the list is empty."
Write-Host "If this fails: .\setup.ps1 or /doctor in Cursor."
cargo run -p mapkeeper-desktop
exit $LASTEXITCODE
