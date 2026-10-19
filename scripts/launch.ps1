param(
    [string]$Workspace = '',
    [string[]]$AllowedRoots = @()
)

$ErrorActionPreference = 'Stop'
$appRoot = Split-Path -Parent $PSScriptRoot
$baseUrl = 'http://127.0.0.1:31080'

function Get-DevFlowExecutable([string]$Root) {
    foreach ($relative in @('devflow.exe', 'target\release\devflow.exe', 'target\debug\devflow.exe')) {
        $candidate = Join-Path $Root $relative
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
    }
    throw '未找到 devflow.exe。请使用完整发布包；源码开发请先编译后端。'
}

function Get-DevFlowHealth {
    try {
        $health = Invoke-RestMethod "$baseUrl/api/health" -TimeoutSec 2
        return ($health.name -eq 'devflow' -and $health.status -eq 'ok')
    } catch { return $false }
}

function Open-DevFlowBrowser {
    $page = Invoke-WebRequest "$baseUrl/" -UseBasicParsing -TimeoutSec 5
    if ($page.StatusCode -ne 200 -or $page.Content -notmatch '<title>DevFlow</title>') {
        throw '后台已运行，但未提供 DevFlow 网页。请使用包含 apps/web/dist 的完整发布包。'
    }
    Start-Process -FilePath $baseUrl
}

function Resolve-DevFlowConfiguration($Config, [string]$SelectedWorkspace, [string[]]$SelectedRoots) {
    if (!$SelectedWorkspace) { $SelectedWorkspace = $env:DEVFLOW_WORKSPACE }
    if (!$SelectedWorkspace -and $Config) { $SelectedWorkspace = $Config.workspace }
    if ($SelectedRoots.Count -eq 0 -and $env:DEVFLOW_ALLOWED_ROOTS) {
        $SelectedRoots = @($env:DEVFLOW_ALLOWED_ROOTS -split ';')
    }
    if ($SelectedRoots.Count -eq 0 -and $Config) { $SelectedRoots = @($Config.allowedRoots) }
    $resolvedRoots = @()
    foreach ($entry in $SelectedRoots) {
        if ([string]::IsNullOrWhiteSpace($entry)) { continue }
        if (!(Test-Path -LiteralPath $entry -PathType Container)) { throw "授权目录不存在：$entry" }
        $resolvedRoots += (Resolve-Path -LiteralPath $entry).Path
    }
    if ($SelectedWorkspace) {
        $SelectedWorkspace = (Resolve-Path -LiteralPath $SelectedWorkspace).Path
        if (Test-Path -LiteralPath $SelectedWorkspace -PathType Container) {
            $resolvedRoots += $SelectedWorkspace
        } elseif ([IO.Path]::GetExtension($SelectedWorkspace) -eq '.code-workspace') {
            $resolvedRoots += Split-Path -Parent $SelectedWorkspace
        } else { throw '初始工作区必须是文件夹或 .code-workspace 文件。' }
    }
    return [pscustomobject]@{
        workspace = $SelectedWorkspace
        allowedRoots = @($resolvedRoots | Select-Object -Unique)
    }
}

function Start-DevFlow {
    $stateDirectory = Join-Path $appRoot '.devflow'
    $configPath = Join-Path $stateDirectory 'launcher.json'
    $mutex = New-Object System.Threading.Mutex($false, 'Local\DevFlow.BrowserLauncher')
    $locked = $false
    try {
        try { $locked = $mutex.WaitOne(30000) } catch [System.Threading.AbandonedMutexException] { $locked = $true }
        if (!$locked) { throw '另一个启动器仍在启动 DevFlow，请稍后重试。' }
        if (Get-DevFlowHealth) {
            Open-DevFlowBrowser
            return
        }
        # Never terminate or replace a process occupying the backend port.
        $probe = New-Object System.Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 31080)
        try { $probe.Start() } catch { throw '端口 31080 已被占用，且不是可用的 DevFlow 服务。请先检查占用程序。' }
        finally { $probe.Stop() }

        $executable = Get-DevFlowExecutable $appRoot
        if (!(Test-Path -LiteralPath (Join-Path $appRoot 'apps\web\dist\index.html') -PathType Leaf)) {
            throw '缺少构建后的网页 apps/web/dist/index.html。请使用完整发布包；源码开发请先执行 pnpm web:build。'
        }
        $config = $null
        if (Test-Path -LiteralPath $configPath) {
            $config = Get-Content -LiteralPath $configPath -Encoding UTF8 -Raw | ConvertFrom-Json
        }
        $config = Resolve-DevFlowConfiguration $config $Workspace $AllowedRoots
        if ($config.allowedRoots.Count -eq 0) {
            Add-Type -AssemblyName System.Windows.Forms
            $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
            $dialog.Description = '首次启动：选择项目或工作区所在目录，DevFlow 仅访问授权目录。'
            try {
                if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { return }
                $config = Resolve-DevFlowConfiguration $null $dialog.SelectedPath @($dialog.SelectedPath)
            } finally { $dialog.Dispose() }
        }
        New-Item -ItemType Directory -Path $stateDirectory -Force | Out-Null
        $json = $config | ConvertTo-Json -Depth 3
        [IO.File]::WriteAllText($configPath, $json, (New-Object Text.UTF8Encoding($false)))
        $env:DEVFLOW_WORKSPACE = $config.workspace
        $env:DEVFLOW_ALLOWED_ROOTS = $config.allowedRoots -join [IO.Path]::PathSeparator
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss-fff'
        $stdout = Join-Path $stateDirectory "backend-$stamp.out.log"
        $stderr = Join-Path $stateDirectory "backend-$stamp.err.log"
        $backend = Start-Process -FilePath $executable -WorkingDirectory $appRoot -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
        $deadline = [DateTime]::UtcNow.AddSeconds(30)
        do {
            $backend.Refresh()
            if ($backend.HasExited) {
                # Another startup path may have won the bind race.
                if (Get-DevFlowHealth) { Open-DevFlowBrowser; return }
                throw "后台启动失败，请查看日志：$stderr"
            }
            if (Get-DevFlowHealth) { Open-DevFlowBrowser; return }
            Start-Sleep -Milliseconds 250
        } while ([DateTime]::UtcNow -lt $deadline)
        throw "等待后台就绪超时，请查看日志：$stderr。后台未被强制结束，可稍后再次双击启动器。"
    } finally {
        if ($locked) { $mutex.ReleaseMutex() }
        $mutex.Dispose()
    }
}

# Dot-source exposes helpers for the focused launcher checks without starting apps.
if ($MyInvocation.InvocationName -ne '.') {
    try { Start-DevFlow } catch {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.MessageBox]::Show($_.Exception.Message, 'DevFlow 启动失败', 'OK', 'Error') | Out-Null
        exit 1
    }
}
