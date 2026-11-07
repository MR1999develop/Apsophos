@echo off
rem Times the network on every compiled back end (TensorRT for RTX, DirectML,
rem CPU) without opening the window; report in target\self-test.txt.
rem Needs a build first (scripts\check.bat). Takes a few minutes; the first
rem time TensorRT for RTX prepares each tile size (about 15 s each).
cd /d "%~dp0.."
if not exist target\release\apsophos.exe (
  echo target\release\apsophos.exe is missing: run scripts\check.bat first.
  pause
  exit /b 1
)
target\release\apsophos.exe --self-test target\self-test.txt
type target\self-test.txt
echo.
pause
