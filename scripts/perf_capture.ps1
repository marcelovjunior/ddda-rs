# Caller must hold the board's live/ddview lease through actual process exit.
# Use a private pinned binary, not Cargo's mutable shared target executable.
# No original game control, input, screenshots or save/install writes.
# Example (PowerShell):
# ./scripts/perf_capture.ps1 -Viewer E:/ddda-re/notes/steps/s41/ddview.exe `
#   -OutputBase E:/ddda-re/notes/steps/s41/field-before -Seconds 60 `
#   -ViewerArgs @('E:/SteamLibrary/steamapps/common/DDDA/nativePC', '--play',
#                '--stage', '100', '--perf', '--unattended', '--size', '1920x1080')
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$Viewer,
    [Parameter(Mandatory)][string]$OutputBase,
    [Parameter(Mandatory)][string[]]$ViewerArgs,
    [ValidateRange(1, 1800)][int]$Seconds = 60,
    [ValidateRange(10, 1800)][int]$WatchdogSeconds = 120,
    [ValidateSet('auto', 'vulkan', 'dx12')][string]$Backend = 'auto',
    [ValidateSet('gpu', 'cpu')][string]$MeshBuilder = 'gpu',
    [ValidateSet('off', 'on')][string]$MaterialSpecialization = 'off',
    [switch]$Trace,
    [switch]$NoShadows
)
$ErrorActionPreference = 'Stop'
if ($WatchdogSeconds -le $Seconds) {
    throw 'WatchdogSeconds must exceed the measured duration and allow startup.'
}
$viewerPath = (Resolve-Path -LiteralPath $Viewer).Path
$outputPath = [IO.Path]::GetFullPath($OutputBase)
$privateRoot = [IO.Path]::GetFullPath('E:/ddda-re') + [IO.Path]::DirectorySeparatorChar
if (-not $outputPath.StartsWith($privateRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Benchmark output must stay in the private research tree.'
}
if (-not $viewerPath.StartsWith($privateRoot, [StringComparison]::OrdinalIgnoreCase) -or
    [IO.Path]::GetFileName($viewerPath) -notlike 'ddview*.exe') {
    throw 'Use a private pinned ddview executable, never the original game.'
}
if (Test-Path -LiteralPath "$outputPath.pid") { throw 'Choose a fresh output name.' }
New-Item -ItemType Directory -Force -Path ([IO.Path]::GetDirectoryName($outputPath)) | Out-Null
# Start-Process joins arguments on Windows. These benchmark args have no quotes
# or trailing backslashes; reject either rather than silently change an argument.
$quotedArguments = foreach ($argument in $ViewerArgs) {
    if ($argument.Contains('"') -or $argument.Contains("`n") -or $argument.EndsWith('\')) {
        throw "Unsupported benchmark argument quoting: $argument"
    }
    '"' + $argument + '"'
}
$oldSeconds = $env:DDVIEW_PERF_SECONDS
$oldTrace = $env:TRACE_CHROME
$oldShadows = $env:DDVIEW_NO_SHADOWS
$oldBackend = $env:WGPU_BACKEND
$oldMeshBuilder = $env:DDVIEW_MESH_BUILDER
$oldMaterialSpecialization = $env:DDVIEW_MATERIAL_SPECIALIZATION
$nvidiaSmi = Get-Command nvidia-smi -ErrorAction SilentlyContinue
$captureProcess = $null
$gpuSamples = [Collections.Generic.List[object]]::new()
try {
    $env:DDVIEW_PERF_SECONDS = [string]$Seconds
    # A trace-feature binary records automatically. Always send its output to
    # the private directory, even if -Trace was omitted; normal builds ignore it.
    $env:TRACE_CHROME = "$outputPath.trace.json"
    $env:DDVIEW_NO_SHADOWS = if ($NoShadows) { '1' } else { $null }
    $env:WGPU_BACKEND = if ($Backend -eq 'auto') { $null } else { $Backend }
    $env:DDVIEW_MESH_BUILDER = $MeshBuilder
    $env:DDVIEW_MATERIAL_SPECIALIZATION = if ($MaterialSpecialization -eq 'on') { '1' } else { $null }
    [ordered]@{
        executable = $viewerPath; sha256 = (Get-FileHash -LiteralPath $viewerPath -Algorithm SHA256).Hash
        arguments = $ViewerArgs; seconds = $Seconds; trace = [bool]$Trace; no_shadows = [bool]$NoShadows
        watchdog_seconds = $WatchdogSeconds
        backend = $Backend; indirect_validation_env = $env:WGPU_VALIDATION_INDIRECT_CALL
        mesh_builder = $MeshBuilder
        material_specialization = $MaterialSpecialization
        started_utc = [DateTime]::UtcNow.ToString('o')
    } | ConvertTo-Json -Depth 4 | Set-Content -LiteralPath "$outputPath.run.json"
    $captureProcess = Start-Process -FilePath $viewerPath -ArgumentList ($quotedArguments -join ' ') `
        -WindowStyle Hidden -PassThru -RedirectStandardOutput "$outputPath.out.txt" `
        -RedirectStandardError "$outputPath.err.txt"
    # Keep this handle for the numeric OS exit code, including a watchdog stop.
    $null = $captureProcess.Handle
    $captureProcess.Id | Set-Content -LiteralPath "$outputPath.pid"
    $watch = [Diagnostics.Stopwatch]::StartNew()
    while (-not $captureProcess.WaitForExit(1000)) {
        if ([int]$watch.Elapsed.TotalSeconds % 5 -eq 0) {
            $engines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine |
                Where-Object { $_.Name -match 'engtype_3D' })
            $original = @(Get-CimInstance Win32_Process -Filter "Name='DDDA.exe'")
            $gpuSamples.Add([ordered]@{
                elapsed_seconds = $watch.Elapsed.TotalSeconds
                utc = [DateTime]::UtcNow.ToString('o')
                original_processes = @($original | Select-Object ProcessId, ExecutablePath)
                engines = @($engines | Select-Object Name, UtilizationPercentage)
                # Optional read-only clock/utilization sample distinguishes GPU
                # work from changing power state. No driver settings are changed.
                nvidia_csv = if ($nvidiaSmi) {
                    & $nvidiaSmi.Source '--query-gpu=timestamp,utilization.gpu,clocks.current.graphics,clocks.current.memory,pstate,power.draw' '--format=csv,noheader,nounits'
                } else { $null }
            })
        }
        if ($watch.Elapsed.TotalSeconds -gt $WatchdogSeconds) {
            $owned = Get-CimInstance Win32_Process -Filter "ProcessId=$($captureProcess.Id)"
            if ($owned -and $owned.ExecutablePath -eq $viewerPath) {
                'Timed exit failed; stopping only the verified owned viewer.' |
                    Set-Content -LiteralPath "$outputPath.watchdog.txt"
                Stop-Process -Id $captureProcess.Id
            } else { throw 'Owned viewer provenance changed; no process was stopped.' }
        }
    }
    $captureProcess.WaitForExit()
    $captureProcess.ExitCode | Set-Content -LiteralPath "$outputPath.exit.txt"
    Write-Output "Viewer PID $($captureProcess.Id) exited $($captureProcess.ExitCode) after $($watch.Elapsed.TotalSeconds) seconds."
} finally {
    $gpuSamples | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath "$outputPath.gpu.json"
    $env:DDVIEW_PERF_SECONDS = $oldSeconds
    $env:TRACE_CHROME = $oldTrace
    $env:DDVIEW_NO_SHADOWS = $oldShadows
    $env:WGPU_BACKEND = $oldBackend
    $env:DDVIEW_MESH_BUILDER = $oldMeshBuilder
    $env:DDVIEW_MATERIAL_SPECIALIZATION = $oldMaterialSpecialization
    if ($captureProcess -and -not $captureProcess.HasExited) {
        Write-Warning "Viewer PID $($captureProcess.Id) still running: retain live/ddview lease until actual exit."
    }
}
if ($captureProcess -and $captureProcess.HasExited) { exit $captureProcess.ExitCode }
