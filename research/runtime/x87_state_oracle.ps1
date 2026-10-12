param([string]$Output = 'E:/ddda-re/notes/native-L46/oracle')
$ErrorActionPreference = 'Stop'
New-Item -ItemType Directory -Force $Output | Out-Null
$Output = (Resolve-Path -LiteralPath $Output).Path
$Source = Join-Path $PSScriptRoot 'x87_state_oracle.c'
$Vc = 'C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars32.bat'
$Batch = Join-Path $Output 'x87_state.cmd'
@"
@echo off
call "$Vc" >nul
cl /nologo /Od "$Source" /Fe:"$Output\x87_state.exe" /Fo:"$Output\x87_state.obj"
if errorlevel 1 exit /b 1
"$Output\x87_state.exe"
"@ | Set-Content -LiteralPath $Batch
& $Batch | Tee-Object -FilePath (Join-Path $Output 'x87_state.log')
if ($LASTEXITCODE -ne 0) { throw "x87_state failed: $LASTEXITCODE" }
