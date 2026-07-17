# Alpha daily path: clean-tree pull + rebuild + launch
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
Set-Location $Root

function Fail-Preflight($Msg) {
    Write-Host $Msg
    Write-Host "First time? run .\setup.ps1"
    Write-Host "Still stuck? run /doctor in Cursor."
    exit 1
}

function Fail-Pull($Msg) {
    Write-Host $Msg
    Write-Host "If this fails, run /doctor in Cursor."
    exit 1
}

function Try-PullIfClean {
    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Write-Host "git not found - skipping pull."
        return
    }

    $status = & git status --porcelain
    if (-not [string]::IsNullOrWhiteSpace($status)) {
        Write-Host "Local changes - skipping pull."
        return
    }

    $upstream = (& git rev-parse --abbrev-ref --symbolic-full-name '@{u}' 2>$null)
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($upstream)) {
        Write-Host "No upstream branch - skipping pull."
        return
    }

    Write-Host "Fetching..."
    & git fetch
    if ($LASTEXITCODE -ne 0) {
        Fail-Pull "git fetch failed."
    }

    $local = (& git rev-parse HEAD).Trim()
    $remote = (& git rev-parse '@{u}').Trim()
    if ($local -eq $remote) {
        Write-Host "Already up to date - skipping pull."
        return
    }

    $before = (& git rev-parse --short HEAD).Trim()
    Write-Host "Pulling (ff-only)..."
    & git pull --ff-only
    if ($LASTEXITCODE -ne 0) {
        Fail-Pull "git pull --ff-only failed."
    }
    $after = (& git rev-parse --short HEAD).Trim()
    Write-Host "Updated: $before -> $after"
}

Write-Host "mapkeeper run (Windows source-run)"
Write-Host "repo: $Root"
Write-Host "Build policy: warnings are errors (same as CI)."
Write-Host ""

# Inherited by the web build and cargo run below.
$env:RUSTFLAGS = "-Dwarnings"

if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
    Fail-Preflight "Not in MAPKEEPER repo root (Cargo.toml missing)."
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail-Preflight "cargo not found."
}

if (-not (Get-Command rustc -ErrorAction SilentlyContinue)) {
    Fail-Preflight "rustc not found."
}

Try-PullIfClean

Write-Host "Building web UI..."
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Web build failed."
    Write-Host "First time? run .\setup.ps1"
    Write-Host "Still stuck? run /doctor in Cursor."
    exit $LASTEXITCODE
}

Write-Host "Launching mapkeeper-desktop..."
Write-Host "After Home opens: use Create your first world if the list is empty."
Write-Host "If this fails: .\setup.ps1 or /doctor in Cursor."
cargo run -p mapkeeper-desktop
exit $LASTEXITCODE
