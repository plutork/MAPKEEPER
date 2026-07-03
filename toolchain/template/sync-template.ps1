# Sync toolchain/template/world/ to mapkeeper-world-template repo root.
# Usage (from MAPKEEPER repo):
#   .\toolchain\template\sync-template.ps1
#   .\toolchain\template\sync-template.ps1 -TargetRepoPath "c:\projects\mapkeeper-world-template" -Push

param(
    [string]$TargetRepoPath = "",
    [switch]$Push,
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

$TemplateDir = $PSScriptRoot
$SourceWorld = Join-Path $TemplateDir "world"
$MapkeeperRoot = (Resolve-Path (Join-Path $TemplateDir "..\..")).Path
$LicenseFile = Join-Path $MapkeeperRoot "LICENSE"

if (-not (Test-Path $SourceWorld)) {
    throw "Source not found: $SourceWorld"
}
if (-not (Test-Path $LicenseFile)) {
    throw "LICENSE not found: $LicenseFile"
}

if ([string]::IsNullOrWhiteSpace($TargetRepoPath)) {
    $SiblingRoot = Split-Path $MapkeeperRoot -Parent
    $TargetRepoPath = Join-Path $SiblingRoot "mapkeeper-world-template"
}

if (-not (Test-Path $TargetRepoPath)) {
    throw "Target repo not found: $TargetRepoPath. Clone mapkeeper-world-template next to MAPKEEPER or pass -TargetRepoPath."
}

$GitDir = Join-Path $TargetRepoPath ".git"
if (-not (Test-Path $GitDir)) {
    throw "Target is not a git repo: $TargetRepoPath"
}

Write-Host "Source:  $SourceWorld"
Write-Host "Target:  $TargetRepoPath"
Write-Host "License: $LicenseFile"

if ($DryRun) {
    Write-Host "[DryRun] robocopy + copy LICENSE"
    exit 0
}

$robocopyArgs = @(
    $SourceWorld,
    $TargetRepoPath,
    "/MIR",
    "/XD", ".git",
    "/NFL", "/NDL", "/NJH", "/NJS", "/NC", "/NS"
)
$rc = & robocopy @robocopyArgs
# robocopy: 0-7 = success
if ($rc -gt 7) {
    throw "robocopy failed with exit code $rc"
}

Copy-Item -Path $LicenseFile -Destination (Join-Path $TargetRepoPath "LICENSE") -Force

Set-Location $TargetRepoPath
git add -A
$status = git status --porcelain
if ([string]::IsNullOrWhiteSpace($status)) {
    Write-Host "Template repo already up to date."
    exit 0
}

$shortSha = (Set-Location $MapkeeperRoot; git rev-parse --short HEAD)
git commit -m "Sync scaffold from MAPKEEPER toolchain/template/world/ ($shortSha)."

if ($Push) {
    git push
    Write-Host "Pushed to origin."
} else {
    Write-Host "Committed locally. Use -Push or push manually."
}
