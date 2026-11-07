@echo off
rem Local checks: format, release build, tests, Clippy and cargo-deny (when
rem installed), with everything on screen. Full output in target\check.log, a
rem summary with the errors of each step in target\check-summary.txt (read by
rem Claude to fix them). Option: -Step fmt^|build^|test^|clippy^|deny.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0check.ps1" %*
pause
