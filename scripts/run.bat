@echo off
rem Starts Apsophos (release build). Drag an image onto this script to open it.
cd /d "%~dp0.."
if not exist target\release\apsophos.exe (
  echo target\release\apsophos.exe is missing: run scripts\check.bat first.
  pause
  exit /b 1
)
start "" target\release\apsophos.exe %*
