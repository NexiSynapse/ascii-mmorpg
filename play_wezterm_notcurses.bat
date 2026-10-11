@echo off
setlocal
cd /d "%~dp0"

set "SCRIPT_DIR=%~dp0"
if "%SCRIPT_DIR:~-1%"=="\" set "SCRIPT_DIR=%SCRIPT_DIR:~0,-1%"

set "WEZTERM_EXE=C:\Program Files\WezTerm\wezterm-gui.exe"
if not exist "%WEZTERM_EXE%" (
    for /f "delims=" %%i in ('where wezterm-gui.exe 2^>nul') do set "WEZTERM_EXE=%%i"
)
if not exist "%WEZTERM_EXE%" (
    for /f "delims=" %%i in ('where wezterm.exe 2^>nul') do set "WEZTERM_EXE=%%i"
)

if not exist "%WEZTERM_EXE%" (
    echo [ERROR] WezTerm executable not found at "%WEZTERM_EXE%" or in PATH.
    echo Please install WezTerm or add it to PATH.
    pause
    exit /b 1
)

echo Launching ASCII MMORPG (Notcurses) inside WezTerm...
start "" "%WEZTERM_EXE%" start --always-new-process --cwd "%SCRIPT_DIR%" -- cmd.exe /k "play_notcurses.bat" %*

