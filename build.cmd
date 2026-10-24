@echo off
setlocal EnableExtensions DisableDelayedExpansion
rem Usage: build.cmd [--check] [--no-pause]
set "BUILD_CHECK_ONLY=0"
set "BUILD_NO_PAUSE=0"
set "BUILD_EXIT_CODE=0"

:parse_args
if "%~1"=="" goto prepare
if /i "%~1"=="--check" (
  set "BUILD_CHECK_ONLY=1"
  shift
  goto parse_args
)
if /i "%~1"=="--no-pause" (
  set "BUILD_NO_PAUSE=1"
  shift
  goto parse_args
)
echo Usage: build.cmd [--check] [--no-pause]
endlocal & exit /b 2

:prepare
pushd "%~dp0"
if errorlevel 1 (
  echo [ERROR] Cannot open the project directory.
  endlocal & exit /b 1
)
rem Match dev.cmd: keep Rust build output outside workspace ACL restrictions.
set "CARGO_TARGET_DIR=%TEMP%\biliscope-target"
echo === BiliScope Windows installer build ===
echo.
where node >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Node.js was not found. Install Node.js 22+ and reopen the terminal.
  goto failed
)
where npm >nul 2>&1
if errorlevel 1 (
  echo [ERROR] npm was not found. Check your Node.js installation.
  goto failed
)
where cargo >nul 2>&1
if errorlevel 1 (
  echo [ERROR] Cargo was not found. Install Rust with the MSVC toolchain.
  goto failed
)
if not exist "node_modules\.bin\tauri.cmd" (
  echo [ERROR] Project dependencies are missing.
  echo Run: npm install --ignore-scripts
  goto failed
)
echo [OK] Basic build dependencies found.
echo Requires Visual Studio C++ Build Tools and Windows SDK.
echo FFmpeg is not bundled. Users must install it before running the installer.
echo Rust output: %CARGO_TARGET_DIR%
if "%BUILD_CHECK_ONLY%"=="1" goto finish
echo.
echo Building frontend, Rust release executable and NSIS installer...
call npm run build:exe
set "BUILD_EXIT_CODE=%ERRORLEVEL%"
if not "%BUILD_EXIT_CODE%"=="0" (
  echo.
  echo [ERROR] Build failed. See the output above.
  goto finish
)
echo.
echo [OK] Build completed.
if not exist "%CARGO_TARGET_DIR%\release\bundle\nsis\*.exe" (
  echo [ERROR] No NSIS installer was generated.
  goto failed
)
if not exist "%~dp0release" mkdir "%~dp0release"
if not exist "%~dp0release" (
  echo [ERROR] Cannot create the release directory.
  goto failed
)
copy /y "%CARGO_TARGET_DIR%\release\bundle\nsis\*.exe" "%~dp0release\" >nul
if errorlevel 1 (
  echo [ERROR] Cannot copy installers to the release directory.
  goto failed
)
rem Workspace files may inherit a Low integrity label from the dev sandbox.
rem Publish directly from Cargo output to the user's normal Downloads folder.
set "BUILD_PUBLISH_DIR=%USERPROFILE%\Downloads\BiliScope-release"
if not exist "%BUILD_PUBLISH_DIR%" mkdir "%BUILD_PUBLISH_DIR%"
if not exist "%BUILD_PUBLISH_DIR%" (
  echo [ERROR] Cannot create the installer publishing directory.
  goto failed
)
copy /y "%CARGO_TARGET_DIR%\release\bundle\nsis\*.exe" "%BUILD_PUBLISH_DIR%\" >nul
if errorlevel 1 (
  echo [ERROR] Cannot publish the installers to Downloads.
  goto failed
)
echo Installer directory - run the installer from here:
echo   %BUILD_PUBLISH_DIR%
echo Project archive copy: %~dp0release
echo.
dir /b "%BUILD_PUBLISH_DIR%\*.exe" 2>nul
goto finish

:failed
set "BUILD_EXIT_CODE=1"

:finish
popd
echo.
if "%BUILD_NO_PAUSE%"=="0" pause
endlocal & exit /b %BUILD_EXIT_CODE%
