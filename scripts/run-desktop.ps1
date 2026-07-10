# agent-managed-alpha-channel (D-80): update-check (ask) → build web → run desktop
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

function Ask-Yes($Prompt) {
    $r = Read-Host "$Prompt [y/N]"
    return ($r -eq "y" -or $r -eq "Y" -or $r -eq "yes")
}

function Test-GitDirty {
    $status = & git status --porcelain
    return -not [string]::IsNullOrWhiteSpace($status)
}

Write-Host "mapkeeper run (Windows source-run)"
Write-Host "repo: $Root"
Write-Host ""

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "cargo not found. Run /mk-install first."
    exit 1
}

$dirty = Test-GitDirty
$updateAvailable = $false
try {
    & git fetch --quiet 2>$null
    $local = (& git rev-parse HEAD).Trim()
    $remote = (& git rev-parse "@{u}" 2>$null)
    if ($LASTEXITCODE -eq 0 -and $remote) {
        $remote = $remote.Trim()
        if ($local -ne $remote) {
            $behind = (& git rev-list --count "HEAD..@{u}" 2>$null)
            if ($behind -and [int]$behind -gt 0) { $updateAvailable = $true }
        }
    }
} catch {
    Write-Host "Update check skipped (no upstream or fetch failed)."
}

if ($updateAvailable) {
    Write-Host "Updates available on the tracked remote branch."
    if ($dirty) {
        Write-Host "Working tree is dirty — will not auto-update. Launching current tree."
        & git status -sb
    } else {
        if (Ask-Yes "Pull updates with --ff-only and rebuild before launch?") {
            & powershell -File (Join-Path $Root "scripts\update-windows.ps1")
            if ($LASTEXITCODE -ne 0) { throw "update failed" }
        } else {
            Write-Host "Skipping update; launching current tree."
        }
    }
}

if (-not (Test-Path (Join-Path $Root "crates\web\dist\index.html"))) {
    Write-Host "Web dist missing — building…"
}

Write-Host "Building web UI…"
powershell -File (Join-Path $Root "crates\web\build.ps1")
if ($LASTEXITCODE -ne 0) { throw "web build failed" }

Write-Host "Launching mapkeeper-desktop…"
Write-Host "After Home opens: use Create your first world if the list is empty."
cargo run -p mapkeeper-desktop
exit $LASTEXITCODE
