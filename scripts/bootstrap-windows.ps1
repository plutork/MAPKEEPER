# agent-managed-alpha-channel (D-80): prepare workspace for source-run
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Test-Cmd($Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
}

function Ask-Yes($Prompt) {
    $r = Read-Host "$Prompt [y/N]"
    return ($r -eq "y" -or $r -eq "Y" -or $r -eq "yes")
}

function Test-WebView2 {
    $keys = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BCC-807D2914E9B6}",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BCC-807D2914E9B6}",
        "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BCC-807D2914E9B6}"
    )
    foreach ($k in $keys) {
        if (Test-Path $k) { return $true }
    }
    return $false
}

Write-Host "mapkeeper bootstrap (Windows) — prepares this Cursor workspace (not a system-wide install)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Test-Cmd "git")) {
    Write-Host "Git is required. Install Git for Windows, then re-run /mk-install."
    exit 1
}

if (-not (Test-Cmd "cargo")) {
    Write-Host "Rust/cargo not found."
    Write-Host "This will run: winget install Rustlang.Rustup  (or open https://rustup.rs )"
    if (-not (Ask-Yes "Install Rust via winget now?")) {
        Write-Host "Stopped. Install Rust manually, restart the terminal, re-run /mk-install."
        exit 1
    }
    winget install -e --id Rustlang.Rustup
    Write-Host "Restart this terminal so cargo is on PATH, then re-run /mk-install."
    exit 1
}

if (-not (Test-Cmd "cl")) {
    Write-Host ""
    Write-Host "MSVC Build Tools (C++ workload) are required and must be installed manually."
    Write-Host "1. Open: https://visualstudio.microsoft.com/visual-cpp-build-tools/"
    Write-Host "2. Install Build Tools with workload 'Desktop development with C++'."
    Write-Host "3. Restart the terminal."
    Write-Host "This script will NOT silent-install MSVC."
    if (-not (Ask-Yes "Confirm MSVC Build Tools are installed and this terminal was restarted?")) {
        Write-Host "Stopped. Finish MSVC setup, then re-run /mk-install."
        exit 1
    }
    if (-not (Test-Cmd "cl")) {
        Write-Host "Still cannot find 'cl'. Open 'x64 Native Tools' / Developer PowerShell and retry."
        exit 1
    }
}

if (-not (Test-WebView2)) {
    Write-Host "WebView2 Runtime not detected."
    Write-Host "Download: https://developer.microsoft.com/microsoft-edge/webview2/"
    if (-not (Ask-Yes "Confirm WebView2 Runtime is installed?")) {
        Write-Host "Stopped. Install WebView2, then re-run /mk-install."
        exit 1
    }
}

$targets = & rustup target list --installed 2>$null
if ($targets -notmatch "wasm32-unknown-unknown") {
    Write-Host "Adding Rust target wasm32-unknown-unknown (needed for web UI)."
    if (-not (Ask-Yes "Run: rustup target add wasm32-unknown-unknown?")) {
        Write-Host "Stopped."
        exit 1
    }
    rustup target add wasm32-unknown-unknown
    if ($LASTEXITCODE -ne 0) { throw "rustup target add failed" }
}

if (-not (Test-Cmd "wasm-bindgen")) {
    Write-Host "wasm-bindgen-cli is required for the web build."
    if (-not (Ask-Yes "Run: cargo install wasm-bindgen-cli --version 0.2.100?")) {
        Write-Host "Stopped."
        exit 1
    }
    cargo install wasm-bindgen-cli --version 0.2.100
    if ($LASTEXITCODE -ne 0) { throw "cargo install wasm-bindgen-cli failed" }
}

Write-Host ""
Write-Host "Building web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) { throw "web build failed" }

Write-Host "Checking desktop crate compiles…"
cargo check -p mapkeeper-desktop
if ($LASTEXITCODE -ne 0) { throw "desktop check failed" }

Write-Host ""
Write-Host "Workspace ready. Next: /mk-run"
exit 0
