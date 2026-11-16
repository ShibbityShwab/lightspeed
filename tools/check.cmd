@echo off
REM LightSpeed pre-push validation from a plain cmd prompt.
REM All logic lives in Check-LightSpeed.ps1 so there is one implementation.
REM
REM Usage:  tools\check.cmd
REM         tools\check.cmd -Fast

setlocal
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Check-LightSpeed.ps1" %*
set EXIT_CODE=%ERRORLEVEL%
exit /b %EXIT_CODE%
