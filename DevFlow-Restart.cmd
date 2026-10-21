@echo off
setlocal
"%SystemRoot%\System32\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -STA -ExecutionPolicy Bypass -File "%~dp0DevFlow-Restart.ps1" %*
set "result=%errorlevel%"
if not "%result%"=="0" (
    echo.
    echo DevFlow restart failed. See the error above.
    pause
)
exit /b %result%
