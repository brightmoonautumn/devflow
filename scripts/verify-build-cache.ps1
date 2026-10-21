# Isolated checks; no compiler, package manager, backend or browser is started.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'build-cache.ps1')
function Assert-Cache($Condition, [string]$Message) { if (!$Condition) { throw $Message } }
$fixture = Join-Path ([IO.Path]::GetTempPath()) ('devflow 中文 build cache ' + [Guid]::NewGuid().ToString('N'))
function Write-Fixture([string]$Relative, [string]$Text) {
    $path = Join-Path $fixture $Relative
    New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
    [IO.File]::WriteAllText($path, $Text, (New-Object Text.UTF8Encoding($false)))
}
try {
    Write-Fixture 'Cargo.toml' '[workspace]'
    Write-Fixture 'Cargo.lock' 'lock'
    Write-Fixture 'crates/core/src/lib.rs' '// 中文'
    Write-Fixture 'apps/web/src/main.ts' '// frontend'
    Write-Fixture 'package.json' '{}'
    Write-Fixture 'pnpm-lock.yaml' 'lock'
    Write-Fixture 'target/debug/devflow.exe' 'exe fixture'
    Write-Fixture 'apps/web/dist/index.html' '<html></html>'
    Write-Fixture 'apps/web/dist/assets/main.js' 'asset'
    $back = Get-DevFlowBuildFingerprint $fixture backend @('rust-a')
    $front = Get-DevFlowBuildFingerprint $fixture frontend @('node-a')
    Assert-Cache ($back -eq (Get-DevFlowBuildFingerprint $fixture backend @('rust-a'))) 'Unchanged source is stable.'
    Assert-Cache ($back -ne (Get-DevFlowBuildFingerprint $fixture backend @('rust-b'))) 'Toolchain changes invalidate.'
    Write-Fixture 'apps/web/src/main.ts' '// changed'
    Assert-Cache ($front -ne (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'Frontend edits invalidate.'
    Assert-Cache ($back -eq (Get-DevFlowBuildFingerprint $fixture backend @('rust-a'))) 'Frontend edit preserves backend cache.'
    $front = Get-DevFlowBuildFingerprint $fixture frontend @('node-a')
    Write-Fixture 'crates/core/src/lib.rs' '// changed'
    Assert-Cache ($back -ne (Get-DevFlowBuildFingerprint $fixture backend @('rust-a'))) 'Backend edits invalidate.'
    Assert-Cache ($front -eq (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'Backend edit preserves frontend cache.'
    Write-Fixture 'apps/web/src/new.ts' '// new'
    Assert-Cache ($front -ne (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'New source invalidates.'
    Remove-Item -LiteralPath (Join-Path $fixture 'apps/web/src/new.ts')
    Assert-Cache ($front -eq (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'Deletion is reflected.'
    Write-Fixture 'apps/web/node_modules/dependency.js' 'ignored'
    Write-Fixture 'apps/web/tsconfig.tsbuildinfo' 'ignored'
    Write-Fixture 'apps/web/dist/assets/new.js' 'generated'
    Assert-Cache ($front -eq (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'Generated files excluded.'
    Write-Fixture 'pnpm-lock.yaml' 'updated lock'
    Assert-Cache ($front -ne (Get-DevFlowBuildFingerprint $fixture frontend @('node-a'))) 'Lock changes invalidate.'
    $cache = Join-Path $fixture '.devflow/build-cache/frontend.json'
    $artifact = Get-DevFlowArtifactFingerprint $fixture frontend
    Assert-Cache (!(Test-DevFlowBuildCurrent $cache $front $artifact)) 'Missing record requires build.'
    Save-DevFlowBuildRecord $cache $front $artifact
    Assert-Cache (Test-DevFlowBuildCurrent $cache $front $artifact) 'Matching record skips build.'
    Assert-Cache (!(Test-DevFlowBuildCurrent $cache 'different' $artifact)) 'Changed input requires build.'
    Write-Fixture 'apps/web/dist/vendor/editor/dist/runtime.js' 'nested generated asset'
    Assert-Cache ($artifact -ne (Get-DevFlowArtifactFingerprint $fixture frontend)) 'Nested dist assets are included.'
    Remove-Item -LiteralPath (Join-Path $fixture 'apps/web/dist/vendor/editor/dist/runtime.js')
    Write-Fixture 'apps/web/dist/assets/main.js' 'changed artifact'
    Assert-Cache (!(Test-DevFlowBuildCurrent $cache $front (Get-DevFlowArtifactFingerprint $fixture frontend))) 'Modified artifact invalidates.'
    Remove-Item -LiteralPath (Join-Path $fixture 'apps/web/dist/index.html')
    Assert-Cache (!(Get-DevFlowArtifactFingerprint $fixture frontend)) 'Missing index invalidates.'
    Write-Fixture '.devflow/build-cache/frontend.json' 'broken JSON'
    Assert-Cache (!(Test-DevFlowBuildCurrent $cache $front $artifact)) 'Corrupt record requires build.'
    Save-DevFlowBuildRecord $cache $front $artifact
    Assert-Cache (Test-DevFlowBuildCurrent $cache $front $artifact) 'Corrupt cache can be replaced.'
    $exe = Get-DevFlowArtifactFingerprint $fixture backend
    Write-Fixture 'target/debug/devflow.exe' 'replacement exe'
    Assert-Cache ($exe -ne (Get-DevFlowArtifactFingerprint $fixture backend)) 'Replacement executable invalidates.'
    Write-Host 'Build cache checks passed.'
} finally {
    $tempPrefix = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
    $absoluteFixture = [IO.Path]::GetFullPath($fixture)
    if (!$absoluteFixture.StartsWith($tempPrefix, [StringComparison]::OrdinalIgnoreCase)) { throw 'Unexpected cleanup path.' }
    if (Test-Path -LiteralPath $absoluteFixture) { Remove-Item -LiteralPath $absoluteFixture -Recurse -Force }
}
