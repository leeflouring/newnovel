@echo off
setlocal
cd /d "%~dp0"

where python >nul 2>nul
if errorlevel 1 (
  echo [ERROR] Python was not found. Please install Python and enable Add Python to PATH.
  pause
  exit /b 1
)

python "novel_similarity_webui.py" --host 127.0.0.1 --port 18080 --open-browser

if errorlevel 1 (
  echo.
  echo [ERROR] Startup failed. Check Python environment or port occupancy.
  pause
)
