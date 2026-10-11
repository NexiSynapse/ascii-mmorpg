@echo off
setlocal
cd /d "%~dp0"

echo Launching ASCII MMORPG (Notcurses) inside WezTerm...
start "" "C:\Program Files\WezTerm\wezterm-gui.exe" start --always-new-process --cwd "%~dp0" -- cmd.exe /k "%~dp0play_notcurses.bat" %*
