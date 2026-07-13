# Headless API smoke for maintainer agents / CI (DEV_AGENT_AUTOMATION).
# Starts mapkeeper-server with a temp fixture world, asserts a few API endpoints, cleans up.
# Usage: .\scripts\smoke-headless.ps1
# Env: SMOKE_PORT (optional fixed port), MAPKEEPER_SMOKE_SKIP_WEB_BUILD=1 to skip wasm rebuild
$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root

$env:RUSTFLAGS = "-Dwarnings"

$TempWorld = $null
$ServerProc = $null

function Cleanup-Smoke {
    if ($null -ne $ServerProc -and -not $ServerProc.HasExited) {
        try {
            Stop-Process -Id $ServerProc.Id -Force -ErrorAction SilentlyContinue
        } catch {}
        try {
            $ServerProc.WaitForExit(5000)
        } catch {}
    }
    if ($null -ne $TempWorld -and (Test-Path -LiteralPath $TempWorld)) {
        try {
            Remove-Item -LiteralPath $TempWorld -Recurse -Force -ErrorAction Stop
        } catch {
            Write-Host "WARN: could not remove temp world: $TempWorld"
        }
    }
}

trap {
    Cleanup-Smoke
    throw $_
}

function Server-BinaryPath {
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root "target" }
    if ($env:OS -eq "Windows_NT") {
        return Join-Path $targetDir "debug\mapkeeper-server.exe"
    }
    return Join-Path $targetDir "debug/mapkeeper-server"
}

function Get-FreeTcpPort {
    $listener = New-Object System.Net.Sockets.TcpListener ([System.Net.IPAddress]::Loopback, 0)
    $listener.Start()
    try {
        return $listener.LocalEndpoint.Port
    } finally {
        $listener.Stop()
    }
}
function Wait-ServerReady([string]$BaseUrl, [int]$Seconds) {
    $deadline = (Get-Date).AddSeconds($Seconds)
    while ((Get-Date) -lt $deadline) {
        if ($null -ne $ServerProc -and $ServerProc.HasExited) {
            throw "mapkeeper-server exited during startup (code $($ServerProc.ExitCode))"
        }
        try {
            $resp = Invoke-WebRequest -Uri "$BaseUrl/api/map" -UseBasicParsing -TimeoutSec 3
            if ($resp.StatusCode -eq 200) {
                return
            }
        } catch {
            Start-Sleep -Milliseconds 400
        }
    }
    throw "Server did not become ready at $BaseUrl within ${Seconds}s"
}

function Assert-JsonGet([string]$Url, [string]$Label) {
    try {
        $resp = Invoke-WebRequest -Uri $Url -UseBasicParsing -TimeoutSec 10
    } catch {
        throw "$Label failed: $($_.Exception.Message)"
    }
    if ($resp.StatusCode -ne 200) {
        throw "$Label failed: HTTP $($resp.StatusCode)"
    }
    if ([string]::IsNullOrWhiteSpace($resp.Content)) {
        throw "$Label failed: empty body"
    }
    $null = $resp.Content | ConvertFrom-Json
}

try {
    if (-not (Test-Path (Join-Path $Root "Cargo.toml"))) {
        Write-Host "Run from MAPKEEPER repo root."
        exit 1
    }

    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Host "cargo not found."
        exit 1
    }

    $FixtureSrc = Join-Path $Root "fixtures\worlds\gentle-plain"
    if (-not (Test-Path (Join-Path $FixtureSrc "mapkeeper.toml"))) {
        Write-Host "Fixture world missing: $FixtureSrc"
        exit 1
    }

    $WebDist = Join-Path $Root "crates\web\dist"
    if ($env:MAPKEEPER_SMOKE_SKIP_WEB_BUILD -ne "1") {
        if (-not (Get-Command wasm-bindgen -ErrorAction SilentlyContinue)) {
            Write-Host "wasm-bindgen not found. Run .\setup.ps1 or set MAPKEEPER_SMOKE_SKIP_WEB_BUILD=1 with existing dist."
            exit 1
        }
        Write-Host "smoke-headless: building web UI..."
        powershell -File (Join-Path $Root "crates\web\build.ps1")
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } elseif (-not (Test-Path $WebDist)) {
        Write-Host "Web dist missing: $WebDist"
        exit 1
    }

    Write-Host "smoke-headless: building mapkeeper-server..."
    cargo build -p mapkeeper-server
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $ServerBin = Server-BinaryPath
    if (-not (Test-Path $ServerBin)) {
        Write-Host "Server binary missing: $ServerBin"
        exit 1
    }

    $TempWorld = Join-Path $env:TEMP ("mapkeeper-smoke-" + [guid]::NewGuid().ToString("n"))
    New-Item -ItemType Directory -Path $TempWorld -Force | Out-Null
    Copy-Item -Path (Join-Path $FixtureSrc "*") -Destination $TempWorld -Recurse -Force

    $Port = if ($env:SMOKE_PORT) { [int]$env:SMOKE_PORT } else { Get-FreeTcpPort }
    $BaseUrl = "http://127.0.0.1:$Port"

    Write-Host "smoke-headless: starting server on port $Port..."
    $ServerProc = Start-Process -FilePath $ServerBin -ArgumentList @(
        "--world", $TempWorld,
        "--port", $Port,
        "--web-dist", $WebDist
    ) -PassThru -WindowStyle Hidden

    Wait-ServerReady $BaseUrl 45

    if ($ServerProc.HasExited) {
        throw "mapkeeper-server exited before smoke assertions (code $($ServerProc.ExitCode))"
    }

    Write-Host "smoke-headless: API assertions..."
    $map = Invoke-WebRequest -Uri "$BaseUrl/api/map" -UseBasicParsing -TimeoutSec 10
    if ($map.StatusCode -ne 200) {
        throw "GET /api/map failed: HTTP $($map.StatusCode)"
    }
    $mapJson = $map.Content | ConvertFrom-Json
    if ($mapJson.world_id -ne "fixture-gentle-plain") {
        throw "GET /api/map unexpected world_id: $($mapJson.world_id)"
    }

    Assert-JsonGet "$BaseUrl/api/integrity" "GET /api/integrity"
    Assert-JsonGet "$BaseUrl/api/layers/elevation" "GET /api/layers/elevation"

    Write-Host "smoke-headless: OK (fixture world, map + integrity + elevation)"
    exit 0
}
finally {
    [void](Cleanup-Smoke)
}
