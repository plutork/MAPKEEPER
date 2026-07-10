# alpha-root-scripts-simplify (D-81): launch only — no update, no git mutation
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
Set-Location $Root

Write-Host "mapkeeper run (Windows source-run)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo not found."
    Write-Host "If this fails, run /doctor in Cursor."
    exit 1
}

Write-Host "Building web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Web build failed."
    Write-Host "If this fails, run /doctor in Cursor."
    exit $LASTEXITCODE
}

Write-Host "Launching mapkeeper-desktop…"
Write-Host "After Home opens: use Create your first world if the list is empty."
Write-Host "If this fails, run /doctor in Cursor."
cargo run -p mapkeeper-desktop
exit $LASTEXITCODE
