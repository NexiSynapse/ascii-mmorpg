@echo off
setlocal
cd /c "%~dp0"

title ASCII MMORPG - Interactive Demo (Optimized)
cls
"%~dp0target\release\ascii-mmorpg-client.exe" --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
