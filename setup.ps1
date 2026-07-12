# alpha-root-scripts-simplify (D-83): first-time workspace bootstrap - not system-wide install
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
Set-Location $Root

function Test-Cmd($Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Ask-Yes($Prompt) {
    $r = Read-Host "$Prompt [y/N]"
    return ($r -eq "y" -or $r -eq "Y" -or $r -eq "yes")
}

function Get-WasmBindgenPin {
    $cargoToml = Join-Path $Root "crates\web\Cargo.toml"
    if (-not (Test-Path $cargoToml)) { return "0.2.100" }
    $line = Select-String -Path $cargoToml -Pattern 'wasm-bindgen\s*=\s*"=?([0-9.]+)"' | Select-Object -First 1
    if ($line -and $line.Matches.Count -gt 0) {
        return $line.Matches[0].Groups[1].Value
    }
    return "0.2.100"
}

Write-Host "mapkeeper setup (Windows) - first-time workspace bootstrap"
Write-Host "repo: $Root"
Write-Host "Not a system-wide app install. No git pull. Asks before heavy changes."
Write-Host "Build policy: warnings are errors (D-97, same as CI)."
Write-Host ""

$env:RUSTFLAGS = "-Dwarnings"

if ($env:OS -ne "Windows_NT") {
    Write-Host "This script is for Windows only."
    Write-Host "If stuck, run /doctor in Cursor."
    exit 1
}

if (-not (Test-Path (Join-Path $Root "Cargo.toml")) -or -not (Test-Path (Join-Path $Root "run.ps1"))) {
    Write-Host "Run this from the MAPKEEPER repo root (Cargo.toml + run.ps1 required)."
    exit 1
}

if (-not (Test-Cmd "git")) {
    Write-Host "Git is required. Install Git for Windows, then re-run .\setup.ps1"
    Write-Host "Or run /doctor in Cursor."
    exit 1
}

if (-not (Test-Cmd "cargo")) {
    Write-Host "Rust/cargo not found."
    Write-Host "Options: https://rustup.rs  or  winget install -e --id Rustlang.Rustup"
    if (-not (Ask-Yes "Install Rust via winget now?")) {
        Write-Host "Stopped. Install Rust, restart the terminal, re-run .\setup.ps1"
        Write-Host "Or run /doctor in Cursor."
        exit 1
    }
    winget install -e --id Rustlang.Rustup
    Write-Host "Restart this terminal so cargo is on PATH, then re-run .\setup.ps1"
    exit 1
}

if (-not (Test-Cmd "rustc") -or -not (Test-Cmd "rustup")) {
    Write-Host "rustc/rustup incomplete on PATH. Restart the terminal after Rust install, or run /doctor."
    exit 1
}

$targets = & rustup target list --installed 2>$null
if ($targets -notcontains "wasm32-unknown-unknown") {
    Write-Host "Adding Rust target wasm32-unknown-unknown (needed for web UI)."
    if (-not (Ask-Yes "Run: rustup target add wasm32-unknown-unknown?")) {
        Write-Host "Stopped. Or run /doctor in Cursor."
        exit 1
    }
    rustup target add wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw "rustup target add failed" }
}

$pin = Get-WasmBindgenPin
if (-not (Test-Cmd "wasm-bindgen")) {
    Write-Host "wasm-bindgen-cli is required (project pin $pin)."
    if (-not (Ask-Yes "Run: cargo install wasm-bindgen-cli --version $pin?")) {
        Write-Host "Stopped. Or run /doctor in Cursor."
        exit 1
    }
    cargo install wasm-bindgen-cli --version $pin
    if ($LASTEXITCODE -ne 0) { throw "cargo install wasm-bindgen-cli failed" }
}

Write-Host ""
Write-Host "Building web UI..."
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) {
    Write-Host "Web build failed. Run /doctor in Cursor."
    exit $LASTEXITCODE
}

if (Ask-Yes "Enable pre-push checks (scripts/check.ps1 - tests + codemap drift)?") {
    & git config core.hooksPath .githooks
    if ($LASTEXITCODE -ne 0) {
        Write-Host "Could not set core.hooksPath — enable manually: git config core.hooksPath .githooks"
    } else {
        Write-Host "Git hooks enabled (.githooks/pre-push)."
    }
} else {
    Write-Host "Skipped hook install. Before push run: .\scripts\check.ps1"
}

Write-Host "Building desktop crate..."
cargo build -p mapkeeper-desktop
if ($LASTEXITCODE -ne 0) {
    Write-Host "Desktop build failed. Run /doctor in Cursor."
    exit $LASTEXITCODE
}

Write-Host ""
Write-Host "Setup OK. Next: .\run.ps1"
exit 0
