# Focused checks only; never execute the restart entry point or stop real processes.
$ErrorActionPreference = 'Stop'
$tokens = $null
$errors = $null
$sourcePath = Join-Path (Split-Path -Parent $PSScriptRoot) 'DevFlow-Restart.ps1'
$ast = [Management.Automation.Language.Parser]::ParseFile($sourcePath, [ref]$tokens, [ref]$errors)
if ($errors.Count) { throw 'Restart script syntax errors.' }
$helper = $ast.Find({ param($node)
    $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Wait-DevFlowPortAvailable'
}, $false)
if (!$helper) { throw 'Port wait helper missing.' }
. ([scriptblock]::Create($helper.Extent.Text))

# Mock only the probe and sleep: no listeners, process mutation or actual delay.
function Test-DevFlowPortAvailable {
    $script:probeCount++
    return ($script:probeCount -gt $script:busyCount)
}
function Start-Sleep { param([int]$Milliseconds) $script:sleepCount++ }

$script:probeCount = 0
$script:sleepCount = 0
$script:busyCount = 0
Wait-DevFlowPortAvailable
if ($script:probeCount -ne 1 -or $script:sleepCount -ne 0) { throw 'Free port must proceed immediately.' }

$script:probeCount = 0
$script:sleepCount = 0
$script:busyCount = 2
Wait-DevFlowPortAvailable
if ($script:probeCount -ne 3 -or $script:sleepCount -ne 2) { throw 'Transient busy port must be retried in the same run.' }

# Force the deadline to expire to verify persistent conflicts remain failures.
$timeoutHelper = $helper.Extent.Text.Replace('AddSeconds(30)', 'AddSeconds(-1)')
. ([scriptblock]::Create($timeoutHelper))
$script:probeCount = 0
$script:busyCount = [int]::MaxValue
$rejected = $false
try { Wait-DevFlowPortAvailable } catch { $rejected = $true }
if (!$rejected) { throw 'Persistent busy port must fail.' }
Write-Host 'Restart port wait checks passed.'
