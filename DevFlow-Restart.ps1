# Source checkout entry point. Windows PowerShell requires UTF-8 with BOM for Chinese.
$ErrorActionPreference = 'Stop'
$appRoot = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\', '/')
$rootPrefix = $appRoot + [IO.Path]::DirectorySeparatorChar
$mutex = New-Object System.Threading.Mutex($false, 'Local\DevFlow.SourceRestart')
$locked = $false

function Test-ProjectBackend([string]$Executable) {
    if (!$Executable) { return $false }
    $absolute = [IO.Path]::GetFullPath($Executable)
    return ([IO.Path]::GetFileName($absolute) -ieq 'devflow.exe' -and
        $absolute.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase))
}

function Resolve-PnpmRunner {
    # Standalone pnpm and npm-installed shims have different executable names.
    foreach ($name in @('pnpm.exe', 'pnpm.cmd', 'pnpm.ps1')) {
        $command = Get-Command $name -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($command) { return [pscustomobject]@{ Path = $command.Source; Arguments = @() } }
    }
    # Node installations may provide Corepack without enabling global pnpm shims.
    # Calling it directly respects package.json's packageManager version.
    foreach ($name in @('corepack.cmd', 'corepack.exe', 'corepack.ps1')) {
        $command = Get-Command $name -CommandType Application,ExternalScript -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($command) { return [pscustomobject]@{ Path = $command.Source; Arguments = @('pnpm') } }
    }
    throw '未找到 pnpm 或 Corepack。请安装 pnpm，或安装带 Corepack 的 Node.js，并确认命令已加入 PATH。'
}

try {
    try { $locked = $mutex.WaitOne(0) } catch [System.Threading.AbandonedMutexException] { $locked = $true }
    if (!$locked) { throw '已有一个更新启动脚本正在执行，请等待它完成。' }
    $cargo = (Get-Command cargo -CommandType Application -ErrorAction Stop).Source
    $pnpm = Resolve-PnpmRunner
    & node (Join-Path $appRoot 'scripts\prepare-ripgrep.cjs')
    if ($LASTEXITCODE -ne 0) { throw 'Unable to prepare search. Run pnpm install with optional dependencies enabled.' }
    Write-Host "前端构建工具：$($pnpm.Path) $($pnpm.Arguments -join ' ')"
    Write-Host '[1/4] 结束本项目已启动的 DevFlow 后台…'
    foreach ($backend in @(Get-CimInstance Win32_Process -Filter "Name = 'devflow.exe'")) {
        if (!(Test-ProjectBackend $backend.ExecutablePath)) { continue }
        # Recheck the executable immediately before stopping the PID.
        $current = Get-CimInstance Win32_Process -Filter "ProcessId = $($backend.ProcessId)"
        if (!$current -or !(Test-ProjectBackend $current.ExecutablePath)) { continue }
        Write-Host "  停止 PID $($current.ProcessId)：$($current.ExecutablePath)"
        Stop-Process -Id $current.ProcessId -Force -ErrorAction Stop
        Wait-Process -Id $current.ProcessId -Timeout 15 -ErrorAction SilentlyContinue
        if (Get-Process -Id $current.ProcessId -ErrorAction SilentlyContinue) {
            throw "后台 PID $($current.ProcessId) 尚未退出，请稍后重试。"
        }
    }
    $probe = New-Object System.Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 31080)
    try { $probe.Start() } catch { throw '端口 31080 仍被占用。为避免结束其他项目的程序，请先检查占用进程。' }
    finally { $probe.Stop() }
    Push-Location -LiteralPath $appRoot
    try {
        Write-Host '[2/4] 编译后端（开发模式）…'
        & $cargo build -p devflow --target-dir (Join-Path $appRoot 'target')
        if ($LASTEXITCODE -ne 0) { throw '后端编译失败，未启动旧版本。' }
        Write-Host '[3/4] 构建前端…'
        $pnpmExecutable = $pnpm.Path
        # Invoke the package directly: root web:build shells out to `pnpm`,
        # which is unavailable when the runner is `corepack pnpm` without shims.
        $pnpmArguments = @($pnpm.Arguments) + @('--filter', '@devflow/web', 'run', 'build')
        & $pnpmExecutable @pnpmArguments
        if ($LASTEXITCODE -ne 0) { throw '前端构建失败，未启动旧版本。' }
        Write-Host '[4/4] 启动新版并打开浏览器…'
        # The normal launcher uses port 31080; ignore a development-shell override.
        $previousPort = $env:DEVFLOW_PORT
        try {
            $env:DEVFLOW_PORT = '31080'
            & (Join-Path $appRoot 'scripts\launch.ps1') -ExecutablePath (Join-Path $appRoot 'target\debug\devflow.exe')
            if ($LASTEXITCODE -ne 0) { throw '启动失败，请查看启动器提示和 .devflow 日志。' }
            $health = Invoke-RestMethod 'http://127.0.0.1:31080/api/health' -TimeoutSec 5
            if ($health.name -ne 'devflow' -or $health.status -ne 'ok') { throw '新版后台健康检查未通过。' }
        } finally { $env:DEVFLOW_PORT = $previousPort }
        Write-Host '已启动：http://127.0.0.1:31080'
    } finally { Pop-Location }
} catch {
    Write-Host "更新启动失败：$($_.Exception.Message)" -ForegroundColor Red
    exit 1
} finally {
    if ($locked) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
