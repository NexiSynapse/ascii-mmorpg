@echo off
setlocal
cd /d "%~dp0"
set "PATH=C:\msys64\ucrt64\bin;C:\msys64\usr\bin;%PATH%"
set "PKG_CONFIG_PATH=C:\msys64\ucrt64\lib\pkgconfig"
set "PKG_CONFIG_ALLOW_CROSS=1"
set "LIBCLANG_PATH=C:\msys64\ucrt64\bin"
chcp 65001 >nul

title ASCII MMORPG - Notcurses Native Demo (Optimized)
echo [1/2] Verifying Notcurses client build and embedded assets...
cargo build --release -p ascii-mmorpg-client --target x86_64-pc-windows-gnu --features notcurses
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo Build failed! Check compiler errors above.
    pause
    exit /b %ERRORLEVEL%
)

cls
"%~dp0target\x86_64-pc-windows-gnu\release\ascii-mmorpg-client.exe" --notcurses --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
