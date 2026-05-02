@echo off
REM LightSpeed pre-push validation script
REM Run this before every commit to catch regressions.
REM
REM Usage:  tools\check.cmd
REM         or just double-click it.

echo ===== LightSpeed Pre-Push Checks =====
echo.

setlocal enabledelayedexpansion
set EXIT_CODE=0

echo [1/6] cargo fmt --all --check
echo ----------------------------------------
call cargo fmt --all --check
if %ERRORLEVEL% neq 0 (
    echo FAILED — run: cargo fmt --all
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo [2/6] cargo clippy (all targets, all features, exclude GUI)
echo ----------------------------------------
call cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
if %ERRORLEVEL% neq 0 (
    echo FAILED — fix clippy warnings
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo [3/6] cargo test (all crates, exclude GUI)
echo ----------------------------------------
call cargo test --workspace --all --exclude lightspeed-gui
if %ERRORLEVEL% neq 0 (
    echo FAILED — fix failing tests
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo [4/6] cargo audit (security advisory check)
echo ----------------------------------------
call cargo audit
if %ERRORLEVEL% neq 0 (
    echo FAILED — run: cargo audit for details
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo [5/6] cargo deny check (licenses + bans + sources)
echo ----------------------------------------
call cargo deny check
if %ERRORLEVEL% neq 0 (
    echo FAILED — run: cargo deny check for details
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo [6/6] cargo build --release (exclude GUI)
echo ----------------------------------------
call cargo build --release --workspace --exclude lightspeed-gui
if %ERRORLEVEL% neq 0 (
    echo FAILED — fix build errors
    set EXIT_CODE=1
) else ( echo ^> PASSED )
echo.

echo ========================================
if %EXIT_CODE% equ 0 (
    echo ALL CHECKS PASSED ^— ready to commit!
) else (
    echo SOME CHECKS FAILED ^— see above for details
)
echo.
exit /b %EXIT_CODE%