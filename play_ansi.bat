@echo off
setlocal
cd /d "%~dp0"

title ASCII MMORPG - Interactive Demo
cls
set "EXE=%~dp0target\release\ascii-mmorpg-client.exe"
if not exist "%EXE%" set "EXE=%~dp0target\debug\ascii-mmorpg-client.exe"
if not exist "%EXE%" (
  echo ERROR: ascii-mmorpg-client.exe not found.
  echo Looked in target\release and target\debug under %~dp0
  echo Run: cargo build -p ascii-mmorpg-client
  echo  or: cargo build --release -p ascii-mmorpg-client
  pause
  exit /b 1
)
echo Using %EXE%
"%EXE%" --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
