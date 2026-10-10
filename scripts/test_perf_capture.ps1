# Offline regressions for the actual harness watchdog/finally AST. No viewer,
# native UI, process query or termination is performed: all process calls below
# use fakes, and Set-Content is intercepted in memory.
$ErrorActionPreference = 'Stop'
$tokens = $null
$parseErrors = $null
$ast = [Management.Automation.Language.Parser]::ParseFile(
    (Join-Path $PSScriptRoot 'perf_capture.ps1'), [ref]$tokens, [ref]$parseErrors)
if ($parseErrors.Count) { throw "Harness parse failed: $parseErrors" }
$watchdog = $ast.Find({ param($node)
    $node -is [Management.Automation.Language.IfStatementAst] -and
    $node.Extent.Text.StartsWith('if ($watch.Elapsed.TotalSeconds -gt $WatchdogSeconds)')
}, $true)
$recordExit = $ast.Find({ param($node)
    $node -is [Management.Automation.Language.IfStatementAst] -and
    $node.Extent.Text.StartsWith('if ($captureProcess -and $captureProcess.HasExited)') -and
    $node.Extent.Text.Contains('.exit.txt')
}, $true)
if (-not $watchdog -or -not $recordExit) { throw 'Harness watchdog/exit recording AST missing' }
$exercise = [scriptblock]::Create('while ($true) {' + $watchdog.Extent.Text + '; break }')
$record = [scriptblock]::Create($recordExit.Extent.Text)

function Get-CimInstance {
    param($ClassName, $Filter)
    $script:queries++
    if ($script:scenario -eq 'exit-during-query') { $script:fake.HasExited = $true }
    if ($script:scenario -like 'foreign-*') {
        return [pscustomobject]@{ ExecutablePath = 'C:/toolchain/rustc.exe' }
    }
    return [pscustomobject]@{ ExecutablePath = $script:expectedPath }
}
function Set-Content {
    param([Parameter(ValueFromPipeline)]$Value, $LiteralPath)
    process { $script:writes.Add([pscustomobject]@{ Path = $LiteralPath; Value = $Value }) }
}

foreach ($case in @('already-exited', 'exit-during-query', 'exit-during-kill',
                   'live-kill-failure', 'foreign-exited-handle', 'foreign-live-handle', 'verified-kill')) {
    $script:scenario = $case
    $script:queries = 0
    $script:expectedPath = 'E:/ddda-re/notes/steps/s41/ddview-test.exe'
    $script:writes = [Collections.Generic.List[object]]::new()
    $script:fake = [pscustomobject]@{
        Id = 1234; HasExited = ($case -eq 'already-exited')
        ExitCode = 17; KillCalls = 0; WaitCalls = 0
    }
    $script:fake | Add-Member ScriptMethod Kill {
        $this.KillCalls++
        if ($script:scenario -eq 'live-kill-failure') { throw 'live stop denied' }
        $this.HasExited = $true
        if ($script:scenario -eq 'exit-during-kill') { throw 'owned handle already exited' }
    }
    $script:fake | Add-Member ScriptMethod WaitForExit {
        param($Milliseconds)
        $this.WaitCalls++
        if ($script:scenario -eq 'foreign-exited-handle') { $this.HasExited = $true }
        return $this.HasExited
    }
    $captureProcess = $script:fake
    $viewerPath = $script:expectedPath
    $outputPath = 'in-memory-test'
    $WatchdogSeconds = 90
    $watch = [pscustomobject]@{ Elapsed = [pscustomobject]@{ TotalSeconds = 100 } }
    $failed = $false
    try { & $exercise } catch { $failed = $true }
    $mustFail = $case -in @('live-kill-failure', 'foreign-live-handle')
    if ($failed -ne $mustFail) { throw "$case wrong failure state: $failed" }
    $expectedKills = if ($case -in @('exit-during-kill', 'live-kill-failure', 'verified-kill')) { 1 } else { 0 }
    if ($script:fake.KillCalls -ne $expectedKills) { throw "$case wrong owned-handle kill count" }
    if ($case -eq 'already-exited' -and $script:queries -ne 0) { throw 'Exited handle must bypass PID query' }
    & $record | Out-Null
    $exitWrites = @($script:writes | Where-Object { $_.Path -like '*.exit.txt' })
    if ($mustFail) {
        if ($exitWrites.Count) { throw "$case recorded a numeric exit while alive" }
    } elseif ($exitWrites.Count -ne 1 -or $exitWrites[0].Value -ne 17) {
        throw "$case did not preserve the actual numeric exit"
    }
    Write-Output "PASS $case"
}
Write-Output '7 offline harness watchdog regressions passed.'
