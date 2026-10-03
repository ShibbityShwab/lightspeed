param([string]$Staged, [string]$Dest = 'C:\Program Files\lightspeed-gui\bin')

# Install the staged build, verifying by CONTENT rather than by exit code.
#
# `Copy-Item -Force` over a RUNNING executable fails on the locked file, but the
# statement still completes, so `$?` is True and the caller sees "RESULT True"
# on an install that never happened. Comparing sizes afterwards is the only
# signal that means anything.

$ErrorActionPreference = 'Stop'
$srcExe = Join-Path $Staged 'lightspeed-gui.exe'
$dstExe = Join-Path $Dest 'lightspeed-gui.exe'

if (-not (Test-Path $srcExe)) { Write-Output "no staged build at $srcExe"; exit 1 }
$wantSize = (Get-Item $srcExe).Length

# Any running instance holds its own exe open, so it must go first. This script
# is expected to run ELEVATED: a medium-integrity shell cannot stop an elevated
# process ("Could not stop process").
Get-Process lightspeed-gui -ErrorAction SilentlyContinue | ForEach-Object {
  Write-Output ("stopping pid {0}" -f $_.Id)
  $_.Kill(); $_.WaitForExit(8000) | Out-Null
}
Start-Sleep -Seconds 3

Copy-Item (Join-Path $Staged '*') $Dest -Force

$gotSize = (Get-Item $dstExe).Length
Write-Output ("staged {0} bytes / installed {1} bytes" -f $wantSize, $gotSize)
if ($gotSize -ne $wantSize) {
  Write-Output 'FAIL: sizes differ, the copy did not land'
  exit 1
}
Write-Output 'OK: installed build matches the staged build'
