@echo off
setlocal
if not exist "%~dp0scripts\launch.ps1" (
    echo Missing scripts\launch.ps1. Please extract the complete DevFlow package.
    pause
    exit /b 1
)
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0scripts\launch.ps1"
exit /b %errorlevel%
