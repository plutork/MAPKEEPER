# Headless shell smoke: health, static UI, identity-only world create.
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
$env:RUSTFLAGS = "-Dwarnings"

$TempRoot = Join-Path $env:TEMP ("mapkeeper-shell-smoke-" + [guid]::NewGuid().ToString("n"))
$ServerProc = $null

function Cleanup-Smoke {
    if ($null -ne $ServerProc -and -not $ServerProc.HasExited) {
        Stop-Process -Id $ServerProc.Id -Force -ErrorAction SilentlyContinue
        try { $ServerProc.WaitForExit(5000) } catch {}
    }
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
}

function Get-FreeTcpPort {
    $listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try { return $listener.LocalEndpoint.Port } finally { $listener.Stop() }
}

function Server-BinaryPath {
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "target" }
    if ($env:OS -eq "Windows_NT") { return Join-Path $targetDir "debug\mapkeeper-server.exe" }
    return Join-Path $targetDir "debug/mapkeeper-server"
}

try {
    New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null
    $env:APPDATA = Join-Path $TempRoot "appdata"
    $env:HOME = Join-Path $TempRoot "home"

    $WebDist = Join-Path $Root "crates\web\dist"
    if ($env:MAPKEEPER_SMOKE_SKIP_WEB_BUILD -ne "1") {
        powershell -File (Join-Path $Root "crates\web\build.ps1")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    cargo build -p mapkeeper-server
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $Port = if ($env:SMOKE_PORT) { [int]$env:SMOKE_PORT } else { Get-FreeTcpPort }
    $BaseUrl = "http://127.0.0.1:$Port"
    $ServerProc = Start-Process -FilePath (Server-BinaryPath) -ArgumentList @(
        "--port", $Port, "--web-dist", $WebDist
    ) -PassThru -WindowStyle Hidden

    $deadline = (Get-Date).AddSeconds(45)
    do {
        try {
            $health = Invoke-RestMethod -Uri "$BaseUrl/api/health" -TimeoutSec 3
            if ($health.status -eq "ok") { break }
        } catch {
            Start-Sleep -Milliseconds 300
        }
    } while ((Get-Date) -lt $deadline)
    if ($health.surface -ne "product-shell") { throw "Health surface mismatch." }

    $html = (Invoke-WebRequest -Uri $BaseUrl -UseBasicParsing -TimeoutSec 10).Content
    foreach ($mode in @("EDITOR", "GENERATOR", "WIZARD", "AGENT", "HISTORY")) {
        if ($html -notmatch $mode) { throw "Missing mode in shell HTML: $mode" }
    }

    $WorldPath = Join-Path $TempRoot "world"
    $body = @{ id = "smoke-world"; path = $WorldPath } | ConvertTo-Json
    $created = Invoke-RestMethod -Uri "$BaseUrl/api/projects" -Method Post -ContentType "application/json" -Body $body
    if ($created.id -ne "smoke-world") { throw "World create returned wrong id." }
    if (-not (Test-Path (Join-Path $WorldPath "mapkeeper.toml"))) { throw "Identity manifest missing." }
    if (Test-Path (Join-Path $WorldPath "map")) { throw "Active create produced archived map contract." }
    if (Test-Path (Join-Path $WorldPath "profiles")) { throw "Active create produced archived profiles contract." }

    Write-Host "smoke-headless: OK (shell modes + identity-only world)"
    exit 0
}
finally {
    [void](Cleanup-Smoke)
}
