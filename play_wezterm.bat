@echo off
setlocal
cd /d "%~dp0"

rem --- Locate WezTerm (override with WEZTERM_EXE if installed elsewhere) ---
set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

if not defined WEZTERM_EXE set "WEZTERM_EXE=C:\Program Files\WezTerm\wezterm-gui.exe"
if not exist "%WEZTERM_EXE%" (
    for /f "delims=" %%i in ('where wezterm-gui.exe 2^>nul') do set "WEZTERM_EXE=%%i"
)
if not exist "%WEZTERM_EXE%" (
    for /f "delims=" %%i in ('where wezterm.exe 2^>nul') do set "WEZTERM_EXE=%%i"
)
if not exist "%WEZTERM_EXE%" if exist "%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe" set "WEZTERM_EXE=%LOCALAPPDATA%\Programs\WezTerm\wezterm-gui.exe"

if not exist "%WEZTERM_EXE%" (
    echo [ERROR] WezTerm executable not found. Install WezTerm, add it to PATH, or set WEZTERM_EXE to its full path.
    pause
    exit /b 1
)

echo Launching ASCII MMORPG inside WezTerm...
start "" "%WEZTERM_EXE%" start --always-new-process --cwd "%SCRIPT_DIR%" -- cmd.exe /k "play_ansi.bat" %*
