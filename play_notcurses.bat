@echo off
setlocal
cd /d "%~dp0"
set "PATH=C:\msys64\ucrt64\bin;C:\msys64\usr\bin;%PATH%"
chcp 65001 >nul

title ASCII MMORPG - Notcurses Native Demo (Optimized)
cls
"%~dp0target\x86_64-pc-windows-gnu\release\ascii-mmorpg-client.exe" --notcurses --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
