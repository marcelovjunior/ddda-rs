param([string]$Output = 'E:/ddda-re/notes/runtime-L27')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Output | Out-Null
$Output = (Resolve-Path -LiteralPath $Output).Path
$Source = Join-Path $PSScriptRoot 'seh_oracle.c'
$Vc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars32.bat'
foreach ($Safe in @($false, $true)) {
    $Name = if ($Safe) { 'seh_safe' } else { 'seh_raw' }
    $Define = if ($Safe) { '/DSAFE_ONLY' } else { '' }
    $Link = if ($Safe) { '/SAFESEH' } else { '/SAFESEH:NO' }
    $Batch = Join-Path $Output "$Name.cmd"
    @"
@echo off
call "$Vc" >nul
cl /nologo /Od /EHsc $Define "$Source" /Fe:"$Output\$Name.exe" /Fo:"$Output\$Name.obj" /link $Link
if errorlevel 1 exit /b 1
"$Output\$Name.exe"
"@ | Set-Content -LiteralPath $Batch
    & $Batch | Tee-Object -FilePath (Join-Path $Output "$Name.log")
    if ($LASTEXITCODE -ne 0) { throw "$Name failed: $LASTEXITCODE" }
    $Lines = Get-Content -LiteralPath (Join-Path $Output "$Name.log")
    foreach ($Required in @(
        'layout record=80 context=716 eip=184 esp=196 eax=176',
        'filter code=e0420000 args=1 value=12345678',
        'selectors cs=23 ss=2b',
        'divide code=c0000094 ip=1 eax=1 ecx=0 edx=0',
        'divide resumed eax=42',
        'direction saved=1 handler=0',
        'arithmetic flags=44',
        'split code=c0000005 kind=1 bad_page=1 prefix=1234',
        'split caught',
        'nonlocal access caught',
        'access code=c0000005 kind=1 address=0000dead ip=1'
    )) {
        if ($Required -cnotin $Lines) { throw "$Name missing expected observation: $Required" }
    }
    if ($Lines -match '^unexpected (inner|outer) return$') { throw 'Nonlocal SEH resumed an abandoned call' }
    if (-not $Safe) {
        $Observed = @($Lines | Where-Object { $_ -match '^handler ' } | ForEach-Object { $_ -replace ' eax=[0-9a-f]+$', '' })
        $Expected = @(
            'handler 1 code=e0420001 flags=0', 'handler 2 code=e0420001 flags=0',
            'handler 1 code=e0420001 flags=0', 'handler 1 code=e0420002 flags=10',
            'handler 2 code=e0420002 flags=0', 'handler 2 code=e0420001 flags=0',
            'handler 1 code=c0000027 flags=2',
            'handler 1 code=e0420001 flags=0', 'handler 3 code=e0420002 flags=0',
            'handler 1 code=e0420002 flags=10', 'handler 2 code=e0420002 flags=0',
            'handler 2 code=e0420001 flags=0',
            'handler 1 code=c0000027 flags=2'
        )
        if (($Observed -join "`n") -cne ($Expected -join "`n")) { throw 'Windows handler order/flags changed' }
        if ('unwind eax=beef' -cnotin $Lines) { throw 'Windows did not restore modified unwind CONTEXT' }
        if (@($Lines | Where-Object { $_ -ceq 'unwind target retained=1' }).Count -ne 2) { throw 'Windows unwind did not retain its target' }
        if (@($Lines | Where-Object { $_ -ceq 'nested link=0' }).Count -ne 5) { throw 'Windows nested exception linkage changed' }
    }
}
Write-Output 'Windows SEH oracle: all comparisons passed'
