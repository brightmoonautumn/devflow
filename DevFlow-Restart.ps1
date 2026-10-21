param([switch]$ForceBuild)

# Source checkout entry point. Windows PowerShell requires UTF-8 with BOM for Chinese.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'scripts/build-cache.ps1')
$appRoot = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\', '/')
$rootPrefix = $appRoot + [IO.Path]::DirectorySeparatorChar
$mutex = New-Object System.Threading.Mutex($false, 'Local\DevFlow.SourceRestart')
$locked = $false
$launcherMutex = New-Object System.Threading.Mutex($false, 'Local\DevFlow.BrowserLauncher')
$launcherLocked = $false
$previousCorepackDefaultToLatest = $env:COREPACK_DEFAULT_TO_LATEST

function Test-ProjectBackend([string]$Executable) {
    if (!$Executable) { return $false }
    $absolute = [IO.Path]::GetFullPath($Executable)
    return ([IO.Path]::GetFileName($absolute) -ieq 'devflow.exe' -and
        $absolute.StartsWith($rootPrefix, [StringComparison]::OrdinalIgnoreCase))
}

function Test-DevFlowPortAvailable {
    $probe = New-Object System.Net.Sockets.TcpListener([Net.IPAddress]::Loopback, 31080)
    try {
        $probe.Server.ExclusiveAddressUse = $true
        $probe.Start()
        return $true
    } catch [System.Net.Sockets.SocketException] {
        if ($_.Exception.SocketErrorCode -ne [Net.Sockets.SocketError]::AddressAlreadyInUse) { throw }
        return $false
    } finally { $probe.Stop() }
}

function Wait-DevFlowPortAvailable {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (!(Test-DevFlowPortAvailable)) {
        if ([DateTime]::UtcNow -ge $deadline) {
            throw '等待 30 秒后端口 31080 仍被占用，请检查占用进程；不会结束其他项目的程序。'
        }
        Start-Sleep -Milliseconds 250
    }
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
    # Older Corepack queries the latest release before reading packageManager.
    # Keep the repository's pinned version and retain signature verification.
    $env:COREPACK_DEFAULT_TO_LATEST = '0'
    Push-Location -LiteralPath $appRoot
    try {
        $pnpmExecutable = $pnpm.Path
        $versionArguments = @($pnpm.Arguments) + @('--version')
        $pnpmVersion = & $pnpmExecutable @versionArguments
        if ($LASTEXITCODE -ne 0) {
            throw 'pnpm 预检查失败，尚未停止后台。若出现 Cannot find matching keyid，请先运行 npm install --global corepack@0.31.0 更新签名密钥，再重试；不要关闭签名校验。'
        }
    } finally { Pop-Location }
    & node (Join-Path $appRoot 'scripts\prepare-ripgrep.cjs')
    if ($LASTEXITCODE -ne 0) { throw 'Unable to prepare search. Run pnpm install with optional dependencies enabled.' }
    Write-Host "前端构建工具：$($pnpm.Path) $($pnpm.Arguments -join ' ')"
    # Fingerprints include uncommitted files and toolchain inputs, not Git HEAD.
    Push-Location -LiteralPath $appRoot
    try {
        $cargoVersion = & $cargo --version
        if ($LASTEXITCODE -ne 0) { throw '无法读取 Cargo 版本。' }
        $rustVersion = & rustc -vV
        if ($LASTEXITCODE -ne 0) { throw '无法读取 Rust 版本。' }
        $nodeVersion = & node --version
        if ($LASTEXITCODE -ne 0) { throw '无法读取 Node.js 版本。' }
        $backendContext = @('cargo build -p devflow --profile dev', $cargo, $cargoVersion) + @($rustVersion)
        $frontendContext = @('pnpm --filter @devflow/web run build', $pnpm.Path, $nodeVersion) + @($pnpmVersion)
        foreach ($entry in Get-ChildItem Env: | Sort-Object Name) {
            if ($entry.Name -match '^(CARGO_|RUST|CC$|CXX$|CFLAGS$|CXXFLAGS$|LDFLAGS$|LIB$|LIBPATH$|INCLUDE$|PATH$)') {
                $backendContext += "$($entry.Name)=$($entry.Value)"
            }
            if ($entry.Name -match '^(VITE_|NODE_|PNPM_|npm_config_|BROWSERSLIST|PATH$)') {
                $frontendContext += "$($entry.Name)=$($entry.Value)"
            }
        }
        # Cargo also reads configuration above the checkout and in CARGO_HOME.
        $cargoConfigs = @()
        $ancestor = [IO.Directory]::GetParent($appRoot)
        while ($ancestor) {
            foreach ($name in @('config', 'config.toml')) { $cargoConfigs += Join-Path $ancestor.FullName ".cargo/$name" }
            $ancestor = $ancestor.Parent
        }
        $cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
        foreach ($name in @('config', 'config.toml')) { $cargoConfigs += Join-Path $cargoHome $name }
        foreach ($config in $cargoConfigs | Select-Object -Unique) {
            if (Test-Path -LiteralPath $config -PathType Leaf) {
                $backendContext += $config + ':' + (Get-DevFlowFileHash $config)
            }
        }
        $backendInput = Get-DevFlowBuildFingerprint $appRoot backend $backendContext
        $frontendInput = Get-DevFlowBuildFingerprint $appRoot frontend $frontendContext
        $backendCache = Join-Path $appRoot '.devflow/build-cache/backend.json'
        $frontendCache = Join-Path $appRoot '.devflow/build-cache/frontend.json'
        $buildBackend = $ForceBuild -or !(Test-DevFlowBuildCurrent $backendCache $backendInput (Get-DevFlowArtifactFingerprint $appRoot backend))
        $buildFrontend = $ForceBuild -or !(Test-DevFlowBuildCurrent $frontendCache $frontendInput (Get-DevFlowArtifactFingerprint $appRoot frontend))
    } finally { Pop-Location }
    # Coordinate with ordinary launchers so they cannot relaunch the old binary.
    try { $launcherLocked = $launcherMutex.WaitOne(30000) } catch [System.Threading.AbandonedMutexException] { $launcherLocked = $true }
    if (!$launcherLocked) { throw '等待普通启动器完成超时，请关闭尚未完成的启动对话框后重试。' }
    Write-Host '[1/4] 结束本项目已启动的 DevFlow 后台…'
    foreach ($backend in @(Get-CimInstance Win32_Process -Filter "Name = 'devflow.exe'")) {
        if (!(Test-ProjectBackend $backend.ExecutablePath)) { continue }
        # Hold the process handle across termination instead of re-querying a PID
        # after exit (the PID can be reused and shutdown releases resources asynchronously).
        $process = Get-Process -Id $backend.ProcessId -ErrorAction SilentlyContinue
        if (!$process) { continue }
        try {
            try { $null = $process.Handle } catch { if ($process.HasExited) { continue }; throw }
            $current = Get-CimInstance Win32_Process -Filter "ProcessId = $($backend.ProcessId)"
            if (!$current -or $current.CreationDate -ne $backend.CreationDate -or
                !(Test-ProjectBackend $current.ExecutablePath)) { continue }
            Write-Host "  停止 PID $($current.ProcessId)：$($current.ExecutablePath)"
            if (!$process.HasExited) {
                try { $process.Kill() } catch { if (!$process.HasExited) { throw } }
            }
            if (!$process.WaitForExit(15000)) {
                throw "后台 PID $($current.ProcessId) 在 15 秒内尚未退出。"
            }
        } finally { $process.Dispose() }
    }
    Write-Host '  等待后台端口释放…'
    Wait-DevFlowPortAvailable
    Push-Location -LiteralPath $appRoot
    try {
        if ($buildBackend) {
            Write-Host '[2/4] 编译后端（开发模式）…'
            if (Test-Path -LiteralPath $backendCache) { Remove-Item -LiteralPath $backendCache }
            & $cargo build -p devflow --target-dir (Join-Path $appRoot 'target')
            if ($LASTEXITCODE -ne 0) { throw '后端编译失败，未启动旧版本。' }
            if ((Get-DevFlowBuildFingerprint $appRoot backend $backendContext) -ne $backendInput) {
                throw '构建期间后端输入发生变化，请重新运行；本次不保存构建记录。'
            }
            Save-DevFlowBuildRecord $backendCache $backendInput (Get-DevFlowArtifactFingerprint $appRoot backend)
        } else { Write-Host '[2/4] 后端无变化，跳过编译。' }
        if ($buildFrontend) {
            Write-Host '[3/4] 构建前端…'
            $pnpmExecutable = $pnpm.Path
            # Invoke directly so Corepack installations without pnpm shims work.
            $pnpmArguments = @($pnpm.Arguments) + @('--filter', '@devflow/web', 'run', 'build')
            if (Test-Path -LiteralPath $frontendCache) { Remove-Item -LiteralPath $frontendCache }
            & $pnpmExecutable @pnpmArguments
            if ($LASTEXITCODE -ne 0) { throw '前端构建失败，未启动旧版本。' }
            if ((Get-DevFlowBuildFingerprint $appRoot frontend $frontendContext) -ne $frontendInput) {
                throw '构建期间前端输入发生变化，请重新运行；本次不保存构建记录。'
            }
            Save-DevFlowBuildRecord $frontendCache $frontendInput (Get-DevFlowArtifactFingerprint $appRoot frontend)
        } else { Write-Host '[3/4] 前端无变化，跳过构建。' }
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
    $env:COREPACK_DEFAULT_TO_LATEST = $previousCorepackDefaultToLatest
    if ($launcherLocked) { $launcherMutex.ReleaseMutex() }
    $launcherMutex.Dispose()
    if ($locked) { $mutex.ReleaseMutex() }
    $mutex.Dispose()
}
