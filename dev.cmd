@echo off
setlocal
cd /d "%~dp0"
set "CARGO_TARGET_DIR=%TEMP%\biliscope-target"
if not defined BILISCOPE_FFMPEG if exist "%~dp0tools\ffmpeg\ffmpeg.exe" set "BILISCOPE_FFMPEG=%~dp0tools\ffmpeg\ffmpeg.exe"

echo Starting BiliScope (Tauri dev)...
call npm run tauri dev
endlocal
