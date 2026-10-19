param([string]$Destination = '')

$ErrorActionPreference = 'Stop'
$appRoot = Split-Path -Parent $PSScriptRoot
if (!$Destination) { $Destination = Join-Path $appRoot ('releases\DevFlow-' + (Get-Date -Format 'yyyyMMdd-HHmmss')) }
$Destination = [IO.Path]::GetFullPath($Destination)
if (Test-Path -LiteralPath $Destination) { throw "输出目录已存在，请选择新目录：$Destination" }
Push-Location $appRoot
try {
    # Building is explicit: normal launcher invocations never install or compile.
    & pnpm web:build
    if ($LASTEXITCODE -ne 0) { throw '前端构建失败。' }
    & cargo build --release -p devflow
    if ($LASTEXITCODE -ne 0) { throw '后端构建失败。' }
    New-Item -ItemType Directory -Path (Join-Path $Destination 'scripts') -Force | Out-Null
    New-Item -ItemType Directory -Path (Join-Path $Destination 'apps\web\dist') -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $appRoot 'target\release\devflow.exe') -Destination (Join-Path $Destination 'devflow.exe')
    Copy-Item -LiteralPath (Join-Path $appRoot 'DevFlow.vbs') -Destination $Destination
    Copy-Item -LiteralPath (Join-Path $appRoot 'DevFlow.cmd') -Destination $Destination
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'launch.ps1') -Destination (Join-Path $Destination 'scripts')
    Get-ChildItem -LiteralPath (Join-Path $appRoot 'apps\web\dist') -Force | Copy-Item -Destination (Join-Path $Destination 'apps\web\dist') -Recurse
    Write-Host "发布包已生成：$Destination"
    Write-Host '将整个文件夹交给使用者，双击 DevFlow.vbs 即可打开浏览器。'
} finally { Pop-Location }
