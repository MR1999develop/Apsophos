@echo off
rem Local checks: `cargo xtask check` (format, build and tests in release and
rem in debug, Clippy, cargo-deny), the same command the CI runs, plus the
rem TensorRT for RTX DLLs next to the program; everything on screen. Full
rem output in target\check.log, a summary with the errors of each step in
rem target\check-summary.txt (read by Claude to fix them).
rem Option: -Step fmt^|build^|test^|clippy^|deny.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0check.ps1" %*
pause
