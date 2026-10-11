@echo off
setlocal
cd /d "%~dp0"

rem --- Locate WezTerm (override with WEZTERM_EXE if installed elsewhere) ---
set "WEZTERM=%WEZTERM_EXE%"
if not defined WEZTERM (
  for /f "delims=" %%i in ('where wezterm-gui.exe 2^>nul') do (
    if not defined WEZTERM set "WEZTERM=%%i"
  )
)
if not defined WEZTERM if exist "%ProgramFiles%\WezTerm\wezterm-gui.exe" set "WEZTERM=%ProgramFiles%\WezTerm\wezterm-gui.exe"
if not defined WEZTERM if exist "%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe" set "WEZTERM=%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe"
if not defined WEZTERM (
  echo ERROR: wezterm-gui.exe not found.
  echo Install WezTerm, add it to PATH, or set WEZTERM_EXE to its full path.
  pause
  exit /b 1
)

echo Launching ASCII MMORPG (Notcurses) inside WezTerm...
start "" "%WEZTERM%" start --always-new-process --cwd "%~dp0" -- cmd.exe /k "%~dp0play_notcurses.bat" %*
