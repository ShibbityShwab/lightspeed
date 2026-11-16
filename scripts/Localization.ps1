<#
.SYNOPSIS
    Run the LightSpeed localization pipeline from Windows PowerShell.

.DESCRIPTION
    Finds a real CPython 3.8+ interpreter, then runs scripts/localization_update.py
    (or only its checks with -Check). This replaces the old bash wrapper: the
    localization logic itself is Python, so no POSIX shell is involved.

    The Microsoft Store aliases `python` and `python3` are not interpreters on
    this host - they exit 9009 with an install prompt. `py` is absent unless the
    official Python installer was used. Nothing here pretends otherwise.

.PARAMETER Check
    Run the four verification gates only; do not regenerate catalogs or pages.

.PARAMETER Python
    Explicit interpreter path or launcher name (for example C:\Python312\python.exe
    or py). When omitted, python3, py and python are probed in that order.

.EXAMPLE
    .\scripts\Localization.ps1 -Check

.EXAMPLE
    .\scripts\Localization.ps1 -Python C:\Python312\python.exe
#>
[CmdletBinding()]
param(
    [switch]$Check,
    [string]$Python
)

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$entry = Join-Path $PSScriptRoot 'localization_update.py'

function Test-PythonCandidate {
    param([string]$Name)
    $command = Get-Command $Name -ErrorAction SilentlyContinue
    if (-not $command) { return $false }
    try {
        $version = & $Name -c 'import sys; print(sys.version_info[0] * 100 + sys.version_info[1])' 2>$null
    } catch {
        return $false
    }
    if ($LASTEXITCODE -ne 0) { return $false }
    if ("$version" -notmatch '^\d+$') { return $false }
    return ([int]$version -ge 308)
}

if (-not $Python) {
    foreach ($candidate in @('python3', 'py', 'python')) {
        if (Test-PythonCandidate -Name $candidate) { $Python = $candidate; break }
    }
}

if (-not $Python) {
    Write-Error @'
No CPython 3.8+ interpreter found (tried python3, py, python).
The Microsoft Store aliases python.exe / python3.exe are stubs that exit 9009;
they are not an interpreter. Install real Python, then reopen the shell:

    winget install Python.Python.3.12

or point -Python at an existing interpreter. CI (ubuntu-latest) has python3.
'@
    exit 1
}

$arguments = @()
if ($Python -eq 'py') { $arguments += '-3' }
$arguments += $entry
if ($Check) { $arguments += '--check' }

Push-Location $root
try {
    & $Python @arguments
    $code = $LASTEXITCODE
} finally {
    Pop-Location
}
if ($code -ne 0) {
    Write-Host "localization failed (exit $code)" -ForegroundColor Red
    exit $code
}
Write-Host 'localization: ok' -ForegroundColor Green
exit 0
