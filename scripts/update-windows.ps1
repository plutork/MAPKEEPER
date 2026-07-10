# agent-managed-alpha-channel (D-80): dirty stop → pull --ff-only → rebuild
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

Write-Host "mapkeeper update (Windows)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
    Write-Host "git not found."
    exit 1
}

$status = & git status --porcelain
if (-not [string]::IsNullOrWhiteSpace($status)) {
    Write-Host "STOP: working tree is dirty. Commit/stash outside alpha agent, or discard locally, then retry."
    & git status -sb
    exit 1
}

$before = (& git rev-parse --short HEAD).Trim()
Write-Host "Before: $before"
Write-Host "Pulling (ff-only)…"
git pull --ff-only
if ($LASTEXITCODE -ne 0) { throw "git pull --ff-only failed" }
$after = (& git rev-parse --short HEAD).Trim()
Write-Host "After:  $after"

Write-Host "Rebuilding web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) { throw "web build failed" }

Write-Host "Checking desktop crate…"
cargo check -p mapkeeper-desktop
if ($LASTEXITCODE -ne 0) { throw "desktop check failed" }

Write-Host ""
Write-Host "Update OK: $before -> $after"
Write-Host "Next: /mk-run"
exit 0
