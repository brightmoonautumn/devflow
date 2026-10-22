@echo off
setlocal
if defined DEVFLOW_PYTHON (
  "%DEVFLOW_PYTHON%" -u "%~dp0scripts\devflow-client.py" %*
) else (
  python -u "%~dp0scripts\devflow-client.py" %*
)
exit /b %errorlevel%
