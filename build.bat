@echo off
rem Build the two binaries into dist\, and optionally install them.
rem
rem   treeserve.exe  - the HTTP server and CLI. Pure Rust, one file, no
rem                    runtime deps beyond the Visual C++ runtime.
rem   treesight.exe  - the desktop app. One file too, but it loads the WebView2
rem                    runtime at run time; that ships with Windows 11 and
rem                    current Windows 10.
rem
rem Neither binary reads anything from disk beyond the folder you point it at:
rem stylesheets, all syntax themes, the Markdown parser and the LaTeX renderer
rem are compiled in, and nothing is fetched at run time.
rem
rem Usage:
rem   build.bat              server + app       -^> dist\
rem   build.bat server       server only        -^> dist\treeserve.exe
rem   build.bat install      copy dist\* to C:\WinApps  (set TREE_PREFIX to
rem                          override)
rem   build.bat bundle       server + app + installers (needs cargo-tauri)
rem
rem To drop the Visual C++ runtime dependency and get a fully self-contained
rem exe, uncomment the next line. Untested here; remove it again if the link
rem step fails, and note that it makes cargo rebuild the dependency tree.
rem set "RUSTFLAGS=-C target-feature=+crt-static"

setlocal
cd /d "%~dp0"

set "target=%~1"
if "%target%"=="" set "target=all"
if "%TREE_PREFIX%"=="" set "TREE_PREFIX=C:\WinApps"

if "%target%"=="all" goto build
if "%target%"=="server" goto build
if "%target%"=="bundle" goto build
if "%target%"=="install" goto install
echo usage: build.bat [all^|server^|install^|bundle] 1>&2
exit /b 2

:build
echo [build] treeserve.exe (server + CLI)
cargo build --release
if errorlevel 1 exit /b 1
if not exist dist md dist
copy /y "target\release\treeserve.exe" "dist\" >nul
if errorlevel 1 exit /b 1

if "%target%"=="server" goto report
echo [build] treesight.exe (desktop app)
cargo build --release -p treesight
if errorlevel 1 exit /b 1
copy /y "target\release\treesight.exe" "dist\" >nul
if errorlevel 1 exit /b 1

if not "%target%"=="bundle" goto report
where cargo-tauri >nul 2>nul
if errorlevel 1 (
    echo error: cargo-tauri not found. Install it with one of: 1>&2
    echo     cargo install tauri-cli --locked 1>&2
    echo     cargo binstall tauri-cli 1>&2
    exit /b 1
)
echo [build] installers
pushd app
cargo tauri build
if errorlevel 1 goto bundlefailed
popd
goto report

:install
if not exist "dist\treeserve.exe" (
    echo nothing in dist\ yet; running a build first
    call "%~f0" all
    if errorlevel 1 exit /b 1
)
rem A trailing space in TREE_PREFIX would silently break every path below.
if "%TREE_PREFIX:~-1%"==" " set "TREE_PREFIX=%TREE_PREFIX:~0,-1%"
if "%TREE_PREFIX:~-1%"=="\" set "TREE_PREFIX=%TREE_PREFIX:~0,-1%"
if not exist "%TREE_PREFIX%" md "%TREE_PREFIX%"
if not exist "%TREE_PREFIX%" (
    echo error: cannot create "%TREE_PREFIX%" ^(try an elevated prompt, or set TREE_PREFIX^) 1>&2
    exit /b 1
)
for %%f in (dist\*.exe) do call :installone "%%f"
if errorlevel 1 exit /b 1
echo.
echo add %TREE_PREFIX% to your PATH if it is not there already
exit /b 0

rem Copies one file and fails loudly, which a bare copy inside the loop did not.
:installone
copy /y %1 "%TREE_PREFIX%" >nul
if errorlevel 1 (
    echo error: could not copy %1 to "%TREE_PREFIX%" 1>&2
    exit /b 1
)
echo [install] %TREE_PREFIX%\%~nx1
exit /b 0

:report
echo.
echo built:
for %%f in (dist\*.exe) do echo     %%f  (%%~zf bytes)
if "%target%"=="bundle" echo     target\release\bundle\ (installers)
echo.
echo install with: build.bat install    (TREE_PREFIX=%TREE_PREFIX%)
exit /b 0

:bundlefailed
popd
exit /b 1
