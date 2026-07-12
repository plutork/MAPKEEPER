# Local CI mirror before push (D-97 + codemap drift D-32).
# Usage: .\scripts\check.ps1
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    Write-Host "Run from MAPKEEPER repo (Cargo.toml missing)."
    exit 1
}

$env:RUSTFLAGS = "-Dwarnings"

Write-Host "check.ps1: cargo test (workspace, exclude desktop)..."
cargo test --workspace --exclude mapkeeper-desktop
if ($LASTEXITCODE -ne 0) {
    Write-Host "Tests failed."
    exit $LASTEXITCODE
}

if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
    Write-Host "python not found - cannot run codemap drift check."
    exit 1
}

Write-Host "check.ps1: codemap drift..."
python (Join-Path $Root "scripts\check_codemap_drift.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix codemap drift:"
    Write-Host "  python scripts/gen_codemap.py"
    Write-Host "  git add docs/CODEMAP.md"
    exit $LASTEXITCODE
}

Write-Host "check.ps1: OK"
exit 0
