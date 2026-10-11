@echo off
setlocal
cd /d "%~dp0"

rem --- Locate MSYS2 UCRT64 runtime (override with MSYS2_ROOT if installed elsewhere) ---
if not defined MSYS2_ROOT set "MSYS2_ROOT=C:\msys64"
if exist "%MSYS2_ROOT%\ucrt64\bin" (
  set "PATH=%MSYS2_ROOT%\ucrt64\bin;%MSYS2_ROOT%\usr\bin;%PATH%"
) else (
  if exist "C:\tools\msys64\ucrt64\bin" (
    set "PATH=C:\tools\msys64\ucrt64\bin;C:\tools\msys64\usr\bin;%PATH%"
  ) else (
    echo WARNING: MSYS2 UCRT64 runtime not found; notcurses DLLs may fail to load.
    echo Set MSYS2_ROOT to your MSYS2 install dir if it is not C:\msys64.
  )
)
chcp 65001 >nul

title ASCII MMORPG - Notcurses Native Demo
cls
rem --- Locate the client exe: prefer release, fall back to debug, any target dir ---
set "EXE="
for %%C in (
  "target\x86_64-pc-windows-gnu\release\ascii-mmorpg-client.exe"
  "target\x86_64-pc-windows-gnu\debug\ascii-mmorpg-client.exe"
  "target\release\ascii-mmorpg-client.exe"
  "target\debug\ascii-mmorpg-client.exe"
) do (
  if not defined EXE (
    if exist "%~dp0%%~C" set "EXE=%~dp0%%~C"
  )
)
if not defined EXE (
  echo ERROR: ascii-mmorpg-client.exe not found.
  echo Looked under %~dp0target for release/debug builds.
  echo Run: cargo build -p ascii-mmorpg-client --features notcurses
  echo  or: cargo build --release -p ascii-mmorpg-client --features notcurses
  pause
  exit /b 1
)
echo Using %EXE%
"%EXE%" --notcurses --fps uncapped %*
echo.
echo Game finished with code %ERRORLEVEL%.
pause
