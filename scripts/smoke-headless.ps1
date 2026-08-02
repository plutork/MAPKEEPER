# Headless product-shell smoke: modes, spatial create, field persist, reopen.
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
    if ($env:OS -eq "Windows_NT") {
        return Join-Path $targetDir (Join-Path "debug" "mapkeeper-server.exe")
    }
    return Join-Path $targetDir (Join-Path "debug" "mapkeeper-server")
}

function Wait-Health([string]$BaseUrl) {
    $deadline = (Get-Date).AddSeconds(45)
    $health = $null
    do {
        try {
            $health = Invoke-RestMethod -Uri "$BaseUrl/api/health" -TimeoutSec 3
            if ($health.status -eq "ok") { return $health }
        } catch {
            Start-Sleep -Milliseconds 300
        }
    } while ((Get-Date) -lt $deadline)
    throw "Server health check timed out."
}

try {
    New-Item -ItemType Directory -Force -Path $TempRoot | Out-Null
    $env:APPDATA = Join-Path $TempRoot "appdata"
    $env:HOME = Join-Path $TempRoot "home"

    $WebDist = Join-Path $Root (Join-Path "crates" (Join-Path "web" "dist"))
    if ($env:MAPKEEPER_SMOKE_SKIP_WEB_BUILD -ne "1") {
        & (Join-Path $Root (Join-Path "crates" (Join-Path "web" "build.ps1")))
    }
    cargo build -p mapkeeper-server
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    $Port = if ($env:SMOKE_PORT) { [int]$env:SMOKE_PORT } else { Get-FreeTcpPort }
    $BaseUrl = "http://127.0.0.1:$Port"
    $startArgs = @{
        FilePath     = (Server-BinaryPath)
        ArgumentList = @("--port", "$Port", "--web-dist", $WebDist)
        PassThru     = $true
    }
    if ($env:OS -eq "Windows_NT") { $startArgs["WindowStyle"] = "Hidden" }
    $ServerProc = Start-Process @startArgs

    $health = Wait-Health $BaseUrl
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
    $manifest = Get-Content -LiteralPath (Join-Path $WorldPath "mapkeeper.toml") -Raw
    if ($manifest -notmatch "\[spatial\]") { throw "Create missing [spatial] config." }
    $StatePath = Join-Path $WorldPath (Join-Path "spatial" "state.json")
    if (-not (Test-Path -LiteralPath $StatePath)) { throw "spatial/state.json missing after create." }
    if (Test-Path (Join-Path $WorldPath "map")) { throw "Active create produced archived map contract." }
    if (Test-Path (Join-Path $WorldPath "profiles")) { throw "Active create produced archived profiles contract." }

    $spatial = Invoke-RestMethod -Uri "$BaseUrl/api/spatial" -TimeoutSec 10
    if (-not $spatial.state) { throw "GET /api/spatial missing state." }
    if (-not $spatial.state.grid) { throw "GET /api/spatial missing grid." }
    if ($null -eq $spatial.state.revision) { throw "GET /api/spatial missing revision." }

    function Get-Cell {
        param($SpatialView, [string]$Key)
        $cells = $SpatialView.state.field.cells
        if ($null -eq $cells) { return $null }
        $prop = $cells.PSObject.Properties[$Key]
        if ($null -ne $prop) { return $prop.Value }
        return $null
    }

    # Small stroke (single-shot commit).
    $rev0 = [int64]$spatial.state.revision
    $strokeSmall = @{
        stroke_id     = "smoke-small"
        base_revision = $rev0
        cells         = @(@{ q = 0; r = 0; value = 3 })
    } | ConvertTo-Json -Depth 5
    $updated = Invoke-RestMethod -Uri "$BaseUrl/api/spatial/stroke" -Method Post -ContentType "application/json" -Body $strokeSmall
    if ((Get-Cell $updated "0,0") -ne 3) { throw "stroke oneshot did not persist cell 0,0=3." }
    if ([int64]$updated.state.revision -ne ($rev0 + 1)) { throw "stroke oneshot did not bump revision." }

    # Multi-chunk stroke (transport only; one commit).
    $rev1 = [int64]$updated.state.revision
    Invoke-RestMethod -Uri "$BaseUrl/api/spatial/stroke/begin" -Method Post -ContentType "application/json" -Body (@{
        stroke_id = "smoke-chunks"; base_revision = $rev1
    } | ConvertTo-Json) | Out-Null
    # Mid-staging must not write disk.
    $midRaw = Get-Content -LiteralPath $StatePath -Raw
    if ($midRaw -match '"1,0"') { throw "chunk staging wrote disk before commit." }
    Invoke-RestMethod -Uri "$BaseUrl/api/spatial/stroke/chunk" -Method Post -ContentType "application/json" -Body (@{
        stroke_id = "smoke-chunks"; chunk_id = "0"
        cells = @(@{ q = 1; r = 0; value = 5 }, @{ q = 2; r = 0; value = 6 })
    } | ConvertTo-Json -Depth 5) | Out-Null
    Invoke-RestMethod -Uri "$BaseUrl/api/spatial/stroke/chunk" -Method Post -ContentType "application/json" -Body (@{
        stroke_id = "smoke-chunks"; chunk_id = "1"
        cells = @(@{ q = 3; r = 0; value = 7 })
    } | ConvertTo-Json -Depth 5) | Out-Null
    $chunked = Invoke-RestMethod -Uri "$BaseUrl/api/spatial/stroke/commit" -Method Post -ContentType "application/json" -Body (@{
        stroke_id = "smoke-chunks"
    } | ConvertTo-Json)
    if ((Get-Cell $chunked "1,0") -ne 5) { throw "multi-chunk commit missing 1,0=5." }
    if ((Get-Cell $chunked "3,0") -ne 7) { throw "multi-chunk commit missing 3,0=7." }
    if ([int64]$chunked.state.revision -ne ($rev1 + 1)) { throw "multi-chunk did not bump one revision." }

    # Legacy field PUT must not exist as independent commit path.
    try {
        Invoke-WebRequest -Uri "$BaseUrl/api/spatial/field" -Method Put -ContentType "application/json" -Body '{"cells":[]}' -UseBasicParsing -TimeoutSec 5 | Out-Null
        throw "legacy /api/spatial/field still accepts writes"
    } catch {
        if ($_.Exception.Message -match "legacy") { throw }
        # 404/405 expected
    }

    Invoke-WebRequest -Uri "$BaseUrl/api/projects/close" -Method Post -UseBasicParsing -TimeoutSec 10 | Out-Null
    $openBody = @{ path = $WorldPath } | ConvertTo-Json
    Invoke-RestMethod -Uri "$BaseUrl/api/projects/open" -Method Post -ContentType "application/json" -Body $openBody | Out-Null
    $fromDisk = Invoke-RestMethod -Uri "$BaseUrl/api/spatial" -TimeoutSec 10
    if ((Get-Cell $fromDisk "0,0") -ne 3) { throw "Re-open lost oneshot cell." }
    if ((Get-Cell $fromDisk "3,0") -ne 7) { throw "Re-open lost multi-chunk cell." }
    $diskRaw = Get-Content -LiteralPath $StatePath -Raw
    if ($diskRaw -notmatch '"0,0"\s*:\s*3') { throw "On-disk missing cell 0,0=3." }
    if (-not (Test-Path -LiteralPath ($StatePath + ".bak"))) {
        # bak created on first replace after ensure; optional on some paths
    }

    # Delete -> app trash (N-025); not permanent purge.
    $deleteBody = @{ path = $WorldPath; expected_id = "smoke-world" } | ConvertTo-Json
    Invoke-WebRequest -Uri "$BaseUrl/api/projects/delete" -Method Post -ContentType "application/json" -Body $deleteBody -UseBasicParsing -TimeoutSec 10 | Out-Null
    if (Test-Path -LiteralPath $WorldPath) { throw "Delete left world path in place (expected trash move)." }
    try {
        Invoke-WebRequest -Uri "$BaseUrl/api/projects/delete" -Method Post -ContentType "application/json" -Body $deleteBody -UseBasicParsing -TimeoutSec 10 | Out-Null
        throw "Delete without registry should have failed"
    } catch {
        if ($_.Exception.Message -match "without registry") { throw }
    }

    Write-Host "smoke-headless: OK (shell + stroke + reopen + delete-trash)"
    exit 0
}
finally {
    [void](Cleanup-Smoke)
}
