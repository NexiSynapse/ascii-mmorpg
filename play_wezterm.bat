@echo off
setlocal
cd /d "%~dp0"

rem --- Locate WezTerm (override with WEZTERM_EXE if installed elsewhere) ---
set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

set "WEZTERM=%WEZTERM_EXE%"
if not defined WEZTERM if exist "%ProgramFiles%\WezTerm\wezterm-gui.exe" set "WEZTERM=%ProgramFiles%\WezTerm\wezterm-gui.exe"
if not defined WEZTERM if exist "C:\Program Files\WezTerm\wezterm-gui.exe" set "WEZTERM=C:\Program Files\WezTerm\wezterm-gui.exe"
if not defined WEZTERM (
  for /f "delims=" %%i in ('where wezterm-gui.exe 2^>nul') do set "WEZTERM=%%i"
)
if not defined WEZTERM (
  for /f "delims=" %%i in ('where wezterm.exe 2^>nul') do set "WEZTERM=%%i"
)
if not defined WEZTERM if exist "%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe" set "WEZTERM=%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe"

if not defined WEZTERM (
  echo [ERROR] wezterm-gui.exe not found. Install WezTerm, add it to PATH, or set WEZTERM_EXE to its full path.
  pause
  exit /b 1
)

echo Launching ASCII MMORPG inside WezTerm...
start "" "%WEZTERM%" start --always-new-process --cwd "%SCRIPT_DIR%" -- cmd.exe /k "play_ansi.bat" %*
