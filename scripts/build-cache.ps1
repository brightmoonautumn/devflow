# Build fingerprints are local optimization records, never a substitute for build success.
function Get-DevFlowFileHash([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $sha = [Security.Cryptography.SHA256]::Create()
    try { return [BitConverter]::ToString($sha.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $sha.Dispose(); $stream.Dispose() }
}

function Get-DevFlowFingerprintFiles([string]$Path, [switch]$IncludeGenerated) {
    if (!(Test-Path -LiteralPath $Path)) { return }
    $item = Get-Item -LiteralPath $Path -Force
    if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) {
        throw "构建指纹不支持符号链接输入：$Path"
    }
    if (!$item.PSIsContainer) { $item.FullName; return }
    foreach ($child in Get-ChildItem -LiteralPath $Path -Force) {
        if (!$IncludeGenerated -and $child.PSIsContainer -and $child.Name -in @('node_modules', 'target', '.git', 'build', 'dist', '.devflow')) { continue }
        if (!$IncludeGenerated -and !$child.PSIsContainer -and $child.Name -like '*.tsbuildinfo') { continue }
        Get-DevFlowFingerprintFiles $child.FullName -IncludeGenerated:$IncludeGenerated
    }
}

function Get-DevFlowManifestHash([string[]]$Files, [string]$Root, [string[]]$Context = @()) {
    $records = New-Object 'System.Collections.Generic.List[string]'
    $records.Add('devflow-build-cache-v1')
    foreach ($entry in $Context) { $records.Add($entry) }
    $ordered = [string[]]@($Files | Select-Object -Unique)
    [Array]::Sort($ordered, [StringComparer]::Ordinal)
    foreach ($file in $ordered) {
        $relative = $file.Substring($Root.Length).Replace('\', '/')
        $records.Add($relative + ':' + (Get-DevFlowFileHash $file))
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes(($records -join "`n"))
        return [BitConverter]::ToString($sha.ComputeHash($bytes)).Replace('-', '').ToLowerInvariant()
    } finally { $sha.Dispose() }
}

function Get-DevFlowBuildFingerprint([string]$Root, [ValidateSet('backend', 'frontend')][string]$Area, [string[]]$Context) {
    $paths = @()
    if ($Area -eq 'backend') {
        $paths += Join-Path $Root 'crates'
        $paths += Join-Path $Root '.cargo'
        $patterns = @('Cargo.toml', 'Cargo.lock', 'rust-toolchain*', 'build.rs', '.env*')
    } else {
        $paths += Join-Path $Root 'apps/web'
        $patterns = @('package.json', 'pnpm-lock.yaml', 'pnpm-workspace.yaml', '.npmrc', '.pnpmfile.cjs', 'tsconfig*.json', '.env*')
    }
    foreach ($file in Get-ChildItem -LiteralPath $Root -File -Force) {
        foreach ($pattern in $patterns) {
            if ($file.Name -like $pattern) { $paths += $file.FullName; break }
        }
    }
    $files = @(foreach ($path in $paths) { Get-DevFlowFingerprintFiles $path })
    Get-DevFlowManifestHash $files $Root (@($Area) + $Context)
}

function Get-DevFlowArtifactFingerprint([string]$Root, [ValidateSet('backend', 'frontend')][string]$Area) {
    if ($Area -eq 'backend') { $path = Join-Path $Root 'target/debug/devflow.exe' }
    else {
        $path = Join-Path $Root 'apps/web/dist'
        if (!(Test-Path -LiteralPath (Join-Path $path 'index.html') -PathType Leaf)) { return $null }
    }
    if (!(Test-Path -LiteralPath $path)) { return $null }
    $files = @(Get-DevFlowFingerprintFiles $path -IncludeGenerated)
    if (!$files.Count) { return $null }
    Get-DevFlowManifestHash $files $Root @($Area)
}

function Test-DevFlowBuildCurrent([string]$CachePath, [string]$InputHash, [string]$ArtifactHash) {
    if (!$ArtifactHash -or !(Test-Path -LiteralPath $CachePath -PathType Leaf)) { return $false }
    try {
        $record = Get-Content -LiteralPath $CachePath -Encoding UTF8 -Raw | ConvertFrom-Json
        return ($record.version -eq 1 -and $record.inputHash -eq $InputHash -and $record.artifactHash -eq $ArtifactHash)
    } catch { return $false }
}

function Save-DevFlowBuildRecord([string]$CachePath, [string]$InputHash, [string]$ArtifactHash) {
    if (!$ArtifactHash) { throw '构建成功但缺少预期产物，不保存构建记录。' }
    $directory = Split-Path -Parent $CachePath
    New-Item -ItemType Directory -Path $directory -Force | Out-Null
    $temporary = Join-Path $directory ([Guid]::NewGuid().ToString('N') + '.tmp')
    $backup = $temporary + '.bak'
    try {
        $record = @{ version = 1; inputHash = $InputHash; artifactHash = $ArtifactHash }
        [IO.File]::WriteAllText($temporary, ($record | ConvertTo-Json), (New-Object Text.UTF8Encoding($false)))
        if (Test-Path -LiteralPath $CachePath) { [IO.File]::Replace($temporary, $CachePath, $backup) }
        else { [IO.File]::Move($temporary, $CachePath) }
    } finally {
        if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        if (Test-Path -LiteralPath $backup) { Remove-Item -LiteralPath $backup }
    }
}
