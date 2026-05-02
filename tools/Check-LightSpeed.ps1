<#
.SYNOPSIS
    LightSpeed pre-push validation script.
.DESCRIPTION
    Runs formatting, clippy, tests, audit, deny, and build checks
    to catch regressions before pushing.
.EXAMPLE
    .\tools\Check-LightSpeed.ps1
#>

$ErrorActionPreference = "Continue"
$exitCode = 0

function Step-Command {
    param($Name, $Command)
    Write-Host "`n[$((++$script:step))/6] $Name" -ForegroundColor Cyan
    Write-Host ("─" * 40) -ForegroundColor DarkGray
    $result = Invoke-Expression $Command
    if ($LASTEXITCODE -eq 0) {
        Write-Host "> PASSED" -ForegroundColor Green
    } else {
        Write-Host "> FAILED — see output above" -ForegroundColor Red
        $script:exitCode = 1
    }
}

Write-Host "===== LightSpeed Pre-Push Checks =====" -ForegroundColor Yellow
$script:step = 0

Step-Command "cargo fmt --all --check" "cargo fmt --all --check 2>&1"
Step-Command "cargo clippy (all targets, all features, exclude GUI)" `
    "cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui 2>&1"
Step-Command "cargo test (all crates, exclude GUI)" `
    "cargo test --workspace --all --exclude lightspeed-gui 2>&1"
Step-Command "cargo audit (security advisory check)" "cargo audit 2>&1"
Step-Command "cargo deny check (licenses + bans + sources)" "cargo deny check 2>&1"
Step-Command "cargo build --release (exclude GUI)" `
    "cargo build --release --workspace --exclude lightspeed-gui 2>&1"

Write-Host "`n========================================" -ForegroundColor Yellow
if ($exitCode -eq 0) {
    Write-Host "ALL CHECKS PASSED — ready to commit!" -ForegroundColor Green
} else {
    Write-Host "SOME CHECKS FAILED — see above for details" -ForegroundColor Red
}
exit $exitCode