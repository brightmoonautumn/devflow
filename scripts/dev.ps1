param([string]$Workspace = "")

$ErrorActionPreference = 'Stop'
$repoRoot = Split-Path -Parent $PSScriptRoot
$backend = $null
if ($Workspace) {
    $resolvedWorkspace = (Resolve-Path -LiteralPath $Workspace).Path
    if (-not (Test-Path -LiteralPath $resolvedWorkspace -PathType Container)) {
        throw 'Workspace must be a directory.'
    }
    $env:DEVFLOW_WORKSPACE = $resolvedWorkspace
}
foreach ($command in @('cargo', 'pnpm')) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "$command is required. See README.md for prerequisites."
    }
}
if (-not (Test-Path -LiteralPath (Join-Path $repoRoot 'node_modules'))) {
    throw 'Run pnpm install from the repository root first.'
}
foreach ($port in @(31080, 5173)) {
    if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) {
        throw "Port $port is already in use. Stop the existing service first."
    }
}
$logDirectory = Join-Path $repoRoot '.devflow'
New-Item -ItemType Directory -Path $logDirectory -Force | Out-Null
Push-Location $repoRoot
try {
    Write-Host 'Starting Rust API at http://127.0.0.1:31080 (first build may take a few minutes)...'
    $backend = Start-Process cargo -ArgumentList 'run', '-p', 'devflow' -WorkingDirectory $repoRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $logDirectory 'backend.out.log') -RedirectStandardError (Join-Path $logDirectory 'backend.err.log')
    $ready = $false
    for ($attempt = 0; $attempt -lt 300; $attempt++) {
        if ($backend.HasExited) {
            throw "Backend exited. See $logDirectory\backend.err.log"
        }
        try {
            $health = Invoke-RestMethod 'http://127.0.0.1:31080/api/health' -TimeoutSec 1
            if ($health.name -eq 'devflow' -and $health.status -eq 'ok') {
                $ready = $true
                break
            }
        } catch { }
        Start-Sleep -Seconds 1
    }
    if (-not $ready) { throw 'Backend startup timed out. Check .devflow/backend.err.log.' }
    Write-Host 'Starting React at http://127.0.0.1:5173. Press Ctrl+C to stop both services.'
    & pnpm web:dev
    if ($LASTEXITCODE -ne 0) { throw "Frontend exited with code $LASTEXITCODE." }
} finally {
    if ($backend -and -not $backend.HasExited) {
        # Stop only the process tree started by this script, including cargo's server child.
        & taskkill /PID $backend.Id /T /F | Out-Null
    }
    Pop-Location
}
