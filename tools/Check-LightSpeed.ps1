<#
.SYNOPSIS
    LightSpeed pre-push validation: the same gates CI runs, on one host.

.DESCRIPTION
    Runs formatting, clippy, tests, the Windows GUI checks, dependency audits and
    a release build. Every step reports PASSED/FAILED and the script exits 1 if
    any step failed.

    CI parity notes:
      - the workspace commands exclude lightspeed-gui, exactly like CI;
      - the GUI crate is linted and tested separately because it only builds on
        Windows and is excluded from every workspace clippy invocation;
      - windows-gui runs these on a runner; this script is the local equivalent.

.PARAMETER SkipGui
    Skip the lightspeed-gui clippy/test steps (they need the Windows-only deps).

.PARAMETER SkipAudit
    Skip cargo audit and cargo deny check (network and advisory database).

.PARAMETER SkipBuild
    Skip the release build.

.PARAMETER Fast
    Shortcut for -SkipAudit -SkipBuild. Keeps fmt, clippy, test and the GUI checks.

.EXAMPLE
    .\tools\Check-LightSpeed.ps1
.EXAMPLE
    .\tools\Check-LightSpeed.ps1 -Fast
#>
[CmdletBinding()]
param(
    [switch]$SkipGui,
    [switch]$SkipAudit,
    [switch]$SkipBuild,
    [switch]$Fast
)

$ErrorActionPreference = 'Continue'
if ($Fast) { $SkipAudit = $true; $SkipBuild = $true }

$steps = New-Object System.Collections.Generic.List[object]
$skipped = New-Object System.Collections.Generic.List[string]

function Test-CargoSubcommand {
    param([string]$Name)
    $null = & cargo $Name --version 2>$null
    return ($LASTEXITCODE -eq 0)
}

function Add-Step {
    param([string]$Name, [scriptblock]$Action)
    $steps.Add([pscustomobject]@{ Name = $Name; Action = $Action })
}

Add-Step 'cargo fmt --all --check' { & cargo fmt --all --check }
Add-Step 'cargo clippy (workspace, all targets/features, excluding GUI)' {
    & cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
}
Add-Step 'cargo test (workspace, excluding GUI)' {
    & cargo test --workspace --all --exclude lightspeed-gui
}
if (-not $SkipGui) {
    Add-Step 'cargo clippy -p lightspeed-gui --all-targets' {
        & cargo clippy -p lightspeed-gui --all-targets
    }
    Add-Step 'cargo test -p lightspeed-gui' { & cargo test -p lightspeed-gui }
}
if (-not $SkipAudit) {
    # cargo-audit and cargo-deny are separate installations, not cargo built-ins.
    # CI owns both gates in its security workflow; locally they are reported as
    # skipped with the install command instead of failing a valid tree.
    if (Test-CargoSubcommand 'audit') {
        Add-Step 'cargo audit' { & cargo audit }
    } else {
        $skipped.Add('cargo audit (install: cargo install cargo-audit)')
    }
    if (Test-CargoSubcommand 'deny') {
        Add-Step 'cargo deny check' { & cargo deny check }
    } else {
        $skipped.Add('cargo deny check (install: cargo install cargo-deny)')
    }
}
if (-not $SkipBuild) {
    Add-Step 'cargo build --release (workspace, excluding GUI)' {
        & cargo build --release --workspace --exclude lightspeed-gui
    }
}

Write-Host '===== LightSpeed pre-push checks =====' -ForegroundColor Yellow
$index = 0
$failures = New-Object System.Collections.Generic.List[string]

foreach ($step in $steps) {
    $index++
    Write-Host ''
    Write-Host ("[{0}/{1}] {2}" -f $index, $steps.Count, $step.Name) -ForegroundColor Cyan
    Write-Host ('-' * 70) -ForegroundColor DarkGray
    $started = Get-Date
    & $step.Action
    $code = $LASTEXITCODE
    $elapsed = [int]((Get-Date) - $started).TotalSeconds
    if ($code -eq 0) {
        Write-Host ("> PASSED ({0}s)" -f $elapsed) -ForegroundColor Green
    } else {
        Write-Host ("> FAILED (exit {0}, {1}s)" -f $code, $elapsed) -ForegroundColor Red
        $failures.Add($step.Name)
    }
}

Write-Host ''
if ($skipped.Count -gt 0) {
    Write-Host 'SKIPPED (not installed locally):' -ForegroundColor DarkYellow
    foreach ($name in $skipped) { Write-Host "  - $name" -ForegroundColor DarkYellow }
}
Write-Host '======================================' -ForegroundColor Yellow
if ($failures.Count -eq 0) {
    Write-Host 'ALL CHECKS PASSED - ready to commit' -ForegroundColor Green
    exit 0
}
Write-Host ("{0} CHECK(S) FAILED:" -f $failures.Count) -ForegroundColor Red
foreach ($name in $failures) { Write-Host "  - $name" -ForegroundColor Red }
exit 1
