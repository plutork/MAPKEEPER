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

Write-Host "check.ps1 [1/10] cargo test (workspace, exclude desktop)..."
cargo test --workspace --exclude mapkeeper-desktop
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix tests: cargo test --workspace --exclude mapkeeper-desktop"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [2/10] clippy workspace incl. tests (-D warnings, excl. desktop)..."
cargo clippy -p mapkeeper-core -p mapkeeper-server -p mapkeeper-web --all-targets -- -D warnings
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix clippy: cargo clippy -p mapkeeper-core -p mapkeeper-server -p mapkeeper-web --all-targets -- -D warnings"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [3/10] codemap drift..."
python (Join-Path $Root "scripts\check_codemap_drift.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix codemap drift:"
    Write-Host "  python scripts/gen_codemap.py"
    Write-Host "  git add docs/CODEMAP.md"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [4/10] archive isolation..."
python (Join-Path $Root "scripts\check_archive_isolation.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix active references to archive/map-v2."
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [5/10] text encoding (mojibake + alpha ps1 ASCII)..."
python (Join-Path $Root "scripts\check_text_encoding.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix encoding: python scripts/check_text_encoding.py"
    Write-Host "  Alpha .ps1 console text must be ASCII only."
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [6/10] doc drift (spatial vs identity-only)..."
python (Join-Path $Root "scripts\check_doc_drift.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix docs: python scripts/check_doc_drift.py"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [7/10] render-scale bench structural (N-026)..."
python (Join-Path $Root "scripts\test_render_scale_bench.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix: scripts/test_render_scale_bench.py (schema / changed_cells)"
    exit $LASTEXITCODE
}
python (Join-Path $Root "scripts\check_render_scale_bench.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix: commit docs/perf/relief-render-scale-report.json + CRS renderer signals"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [8/10] web shell modules structural (N-027)..."
python (Join-Path $Root "scripts\check_web_shell_modules.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix: crates/web ES modules + thin index.html per N-027"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [9/10] domain constant parity (N-030)..."
python (Join-Path $Root "scripts\check_domain_constants.py")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix: shell mirrors must match core constants; gesture rule from probe_next_relief"
    exit $LASTEXITCODE
}

Write-Host "check.ps1 [10/10] web shell pure unit (N-027)..."
node (Join-Path $Root "scripts\web-shell-unit.mjs")
if ($LASTEXITCODE -ne 0) {
    Write-Host ""
    Write-Host "Fix: node scripts/web-shell-unit.mjs"
    exit $LASTEXITCODE
}

if ($Smoke) {
    Write-Host "check.ps1 [smoke] headless spatial smoke (opt-in)..."
    powershell -File (Join-Path $Root "scripts\smoke-headless.ps1")
    if ($LASTEXITCODE -ne 0) {
        Write-Host ""
        Write-Host "Fix smoke: .\scripts\smoke-headless.ps1"
        exit $LASTEXITCODE
    }
}

Write-Host "check.ps1: OK (CI parity except desktop build)$(if ($Smoke) { ' + smoke' } else { '' })"
exit 0
