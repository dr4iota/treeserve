@echo off
rem Build both binaries into dist\.
rem
rem   treeserve.exe  - the HTTP server and CLI. No system dependencies.
rem   treesight.exe  - the desktop app. Needs the WebView2 runtime, which is
rem                    preinstalled on Windows 11 and current Windows 10.
rem
rem Both binaries are self-contained: stylesheets, syntax themes and the
rem Markdown and math renderers are compiled in, and nothing is fetched at
rem run time.
rem
rem Usage:
rem   build.bat            server + app
rem   build.bat server     server only
rem   build.bat bundle     server + app + installers (needs cargo-tauri)

setlocal
cd /d "%~dp0"

set "target=%~1"
if "%target%"=="" set "target=all"
if "%target%"=="all" goto ok
if "%target%"=="server" goto ok
if "%target%"=="bundle" goto ok
echo usage: build.bat [all^|server^|bundle] 1>&2
exit /b 2

:ok
echo [1/2] treeserve.exe (server + CLI)
cargo build --release
if errorlevel 1 exit /b 1

if "%target%"=="server" goto collect
echo [2/2] treesight.exe (desktop app)
cargo build --release -p treesight
if errorlevel 1 exit /b 1

:collect
if not exist dist md dist
copy /y "target\release\treeserve.exe" "dist\" >nul
if errorlevel 1 exit /b 1
if "%target%"=="server" goto done
copy /y "target\release\treesight.exe" "dist\" >nul
if errorlevel 1 exit /b 1

if not "%target%"=="bundle" goto done
where cargo-tauri >nul 2>nul
if errorlevel 1 (
    echo error: cargo-tauri not found. Install it with one of: 1>&2
    echo     cargo install tauri-cli --locked 1>&2
    echo     cargo binstall tauri-cli 1>&2
    exit /b 1
)
echo [3/3] installers
rem Downloads the NSIS tooling on first run; installers land in
rem target\release\bundle\.
pushd app
cargo tauri build
if errorlevel 1 goto bundlefailed
popd

:done
echo.
echo built:
echo     dist\treeserve.exe
if not "%target%"=="server" echo     dist\treesight.exe
if "%target%"=="bundle" echo     target\release\bundle\ (installers)
exit /b 0

:bundlefailed
popd
exit /b 1
