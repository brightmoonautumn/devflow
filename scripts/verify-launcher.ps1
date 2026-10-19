# Focused helper checks only: no browser launch, backend startup, build or dependencies.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'launch.ps1')

function Assert-Launcher($Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
}

$fixtureParent = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixture = Join-Path $fixtureParent ('devflow-launcher-' + [Guid]::NewGuid().ToString('N'))
$savedWorkspace = $env:DEVFLOW_WORKSPACE
$savedRoots = $env:DEVFLOW_ALLOWED_ROOTS
try {
    $env:DEVFLOW_WORKSPACE = ''
    $env:DEVFLOW_ALLOWED_ROOTS = ''
    $projectDirectory = Join-Path $fixture '中文 空格项目'
    New-Item -ItemType Directory -Path $projectDirectory -Force | Out-Null
    $workspaceFile = Join-Path $fixture '中文 工作区.code-workspace'
    [IO.File]::WriteAllText($workspaceFile, '{"folders":[]}', (New-Object Text.UTF8Encoding($false)))

    $config = Resolve-DevFlowConfiguration $null $workspaceFile @($projectDirectory)
    Assert-Launcher ($config.workspace -eq $workspaceFile) '保留中文工作区文件路径'
    Assert-Launcher ($config.allowedRoots -contains $fixture) '工作区文件父目录纳入授权'
    Assert-Launcher ($config.allowedRoots -contains $projectDirectory) '保留显式授权目录'

    $config = Resolve-DevFlowConfiguration $null $projectDirectory @($projectDirectory, $projectDirectory)
    Assert-Launcher ($config.allowedRoots.Count -eq 1) '重复根目录去重'
    $configFile = Join-Path $fixture 'launcher.json'
    [IO.File]::WriteAllText($configFile, ($config | ConvertTo-Json -Depth 3), (New-Object Text.UTF8Encoding($false)))
    $saved = Get-Content -LiteralPath $configFile -Encoding UTF8 -Raw | ConvertFrom-Json
    $restored = Resolve-DevFlowConfiguration $saved '' @()
    Assert-Launcher ($restored.workspace -eq $projectDirectory) '恢复保存的 UTF-8 启动配置'

    $env:DEVFLOW_WORKSPACE = $fixture
    $env:DEVFLOW_ALLOWED_ROOTS = $fixture
    $fromEnvironment = Resolve-DevFlowConfiguration $saved '' @()
    Assert-Launcher ($fromEnvironment.workspace -eq $fixture) '环境配置优先于保存配置'
    $explicit = Resolve-DevFlowConfiguration $saved $projectDirectory @($projectDirectory)
    Assert-Launcher ($explicit.workspace -eq $projectDirectory) '显式参数优先于环境配置'
    $env:DEVFLOW_WORKSPACE = ''
    $env:DEVFLOW_ALLOWED_ROOTS = ''

    $rejected = $false
    try { Resolve-DevFlowConfiguration $null '' @((Join-Path $fixture 'missing')) | Out-Null } catch { $rejected = $true }
    Assert-Launcher $rejected '拒绝不存在的授权目录'
    $rejected = $false
    try { Get-DevFlowExecutable $fixture | Out-Null } catch { $rejected = $true }
    Assert-Launcher $rejected '缺少后台程序时明确失败'
    $debugDirectory = Join-Path $fixture 'target\debug'
    $releaseDirectory = Join-Path $fixture 'target\release'
    New-Item -ItemType Directory -Path $debugDirectory, $releaseDirectory -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $debugDirectory 'devflow.exe'), '')
    Assert-Launcher ((Get-DevFlowExecutable $fixture) -eq (Join-Path $debugDirectory 'devflow.exe')) '源码调试程序兜底'
    [IO.File]::WriteAllText((Join-Path $releaseDirectory 'devflow.exe'), '')
    Assert-Launcher ((Get-DevFlowExecutable $fixture) -eq (Join-Path $releaseDirectory 'devflow.exe')) '优先使用 release 程序'
    [IO.File]::WriteAllText((Join-Path $fixture 'devflow.exe'), '')
    Assert-Launcher ((Get-DevFlowExecutable $fixture) -eq (Join-Path $fixture 'devflow.exe')) '优先使用发布包程序'
    Write-Host 'Launcher helper checks passed.'
} finally {
    $env:DEVFLOW_WORKSPACE = $savedWorkspace
    $env:DEVFLOW_ALLOWED_ROOTS = $savedRoots
    $resolvedFixture = [IO.Path]::GetFullPath($fixture)
    if ($resolvedFixture.StartsWith($fixtureParent, [StringComparison]::OrdinalIgnoreCase) -and
        [IO.Path]::GetFileName($resolvedFixture) -match '^devflow-launcher-[a-f0-9]{32}$') {
        if (Test-Path -LiteralPath $resolvedFixture) { Remove-Item -LiteralPath $resolvedFixture -Recurse -Force }
    } else { throw '拒绝清理不属于本次测试的路径。' }
}
