# Local CI mirror for the active product shell.
# Usage: .\scripts\check.ps1 [-Smoke]
param(
    [switch]$Smoke
)

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    Write-Host "Run from MAPKEEPER repo (Cargo.toml missing)."
    exit 1
}

$env:RUSTFLAGS = "-Dwarnings"

function Require-Python {
    if (-not (Get-Command python -ErrorAction SilentlyContinue)) {
        Write-Host "python not found - required for codemap/isolation/encoding checks."
        exit 1
    }
}

Require-Python

Write-Host "check.ps1 [1/5] cargo test (workspace, exclude desktop)..."
cargo test --workspace --exclude mapkeeper-desktop
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix tests: cargo test --workspace --exclude mapkeeper-desktop"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [2/5] clippy workspace (-D warnings, excl. desktop)..."
cargo clippy -p mapkeeper-core -p mapkeeper-server -p mapkeeper-web -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix clippy: cargo clippy -p mapkeeper-core -p mapkeeper-server -p mapkeeper-web -- -D warnings"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [3/5] codemap drift..."
python (Join-Path $Root "scripts\check_codemap_drift.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix codemap drift:"
    Write-Host "  python scripts/gen_codemap.py"
    Write-Host "  git add docs/CODEMAP.md"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [4/5] archive isolation..."
python (Join-Path $Root "scripts\check_archive_isolation.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix active references to archive/map-v2."
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [5/5] text encoding (mojibake + alpha ps1 ASCII)..."
python (Join-Path $Root "scripts\check_text_encoding.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix encoding: python scripts/check_text_encoding.py"
    Write-Host "  Alpha .ps1 console text must be ASCII only."
    exit $LASTEXITCODE
}

if ($Smoke) {
    Write-Host "check.ps1 [smoke] headless API smoke (opt-in)..."
    powershell -File (Join-Path $Root "scripts\smoke-headless.ps1")
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "Fix smoke: .\scripts\smoke-headless.ps1"
        exit $LASTEXITCODE
    }
}

Write-Host "check.ps1: OK (CI parity except desktop build)$(if ($Smoke) { ' + smoke' } else { '' })"
exit 0
