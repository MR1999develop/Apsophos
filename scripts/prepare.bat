@echo off
rem One-off preparation: FP16 models for TensorRT for RTX, where TensorRT for
rem RTX 1.4 is, and with -RepairDlls ONNX Runtime's DLLs fetched again (see
rem scripts\prepare.ps1). Log in target\prepare.log.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0prepare.ps1" %*
pause
