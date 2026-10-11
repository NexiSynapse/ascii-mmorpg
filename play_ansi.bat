@echo off
setlocal
cd /d "%~dp0"

title ASCII MMORPG - Interactive Demo (Optimized)
echo [1/2] Verifying client build and embedded assets...
cargo build --release -p ascii-mmorpg-client
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo Build failed! Check compiler errors above.
    pause
    exit /b %ERRORLEVEL%
)

cls
"%~dp0target\release\ascii-mmorpg-client.exe" --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
