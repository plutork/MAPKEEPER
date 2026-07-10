# alpha-root-scripts-simplify (D-81): explicit update — dirty stop, pull, rebuild (no launch)
# Daily path: .\run.ps1 (D-86 — pull when clean + launch)
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
Set-Location $Root

Write-Host "mapkeeper update (Windows)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "git not found."
    Write-Host "If this fails, run .\setup.ps1 or /doctor in Cursor."
    exit 1
}

$status = & git status --porcelain
if (-not [string]::IsNullOrWhiteSpace($status)) {
    Write-Host "STOP: working tree is dirty. Resolve locally, then retry."
    & git status -sb
    exit 1
}

$before = (& git rev-parse --short HEAD).Trim()
Write-Host "Before: $before"
Write-Host "Pulling (ff-only)…"
git pull --ff-only
if ($LASTEXITCODE -ne 0) {
    Write-Host "git pull --ff-only failed."
    Write-Host "If this fails, run /doctor in Cursor."
    exit $LASTEXITCODE
}
$after = (& git rev-parse --short HEAD).Trim()
Write-Host "After:  $after"

Write-Host "Rebuilding web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Web build failed."
    Write-Host "If this fails, run .\setup.ps1 or /doctor in Cursor."
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "Update OK: $before -> $after"
Write-Host "Next: .\run.ps1 (pull+launch) or launch only if already built."
exit 0
