# agent-managed-alpha-channel (D-80): read-only diagnostics
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Test-Cmd($Name) {
    return [bool](Get-Command $Name -ErrorAction SilentlyContinue)
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

Write-Host "mapkeeper doctor (Windows)"
Write-Host "repo: $Root"
Write-Host ""

$ok = $true
function Show-Check($Label, $Pass, $Hint) {
    if ($Pass) {
        Write-Host "[OK] $Label"
    } else {
        Write-Host "[!!] $Label — $Hint"
        $script:ok = $false
    }
}

Show-Check "git" (Test-Cmd "git") "Install Git for Windows"
Show-Check "rustc" (Test-Cmd "rustc") "Install Rust (rustup) — use /mk-install"
Show-Check "cargo" (Test-Cmd "cargo") "Install Rust (rustup) — use /mk-install"

$msvc = Test-Cmd "cl"
Show-Check "MSVC linker (cl)" $msvc "Install Visual Studio Build Tools (C++ workload) manually, then confirm in /mk-install"

$wasm = $false
if (Test-Cmd "rustup") {
    $targets = & rustup target list --installed 2>$null
    if ($targets -match "wasm32-unknown-unknown") { $wasm = $true }
}
Show-Check "wasm32-unknown-unknown" $wasm "rustup target add wasm32-unknown-unknown — use /mk-install"

Show-Check "WebView2 runtime" (Test-WebView2) "Install Microsoft Edge WebView2 Runtime"
Show-Check "web dist" (Test-Path (Join-Path $Root "crates\web\dist\index.html")) "Missing — /mk-install or /mk-run will build it"

Write-Host ""
if ($ok) {
    Write-Host "Next: /mk-run"
    exit 0
} else {
    Write-Host "Next: /mk-install"
    exit 1
}
