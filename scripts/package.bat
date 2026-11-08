@echo off
rem Builds the ready-to-use package of Apsophos for Windows:
rem   dist\Apsophos-<version>-windows-x64\   (folder to unzip and run)
rem   dist\Apsophos-<version>-windows-x64.zip
rem It takes the program and DLLs that scripts\check.bat put in target\release,
rem the models in models\ and the licence texts. Run check.bat first.
rem   scripts\package.bat            with TensorRT for RTX (NVIDIA DLLs included)
rem   scripts\package.bat nonvidia   without the NVIDIA DLLs (DirectML and CPU only)
rem   scripts\package.bat full       with every tile size (both options can be given)
rem With Inno Setup 6 installed it also makes dist\Apsophos-<version>-windows-x64-setup.exe.
rem Every model file holds all the network's weights, so the package carries
rem only the sizes the tile search picks in practice: 448 (CPU and small
rem GPUs), 960 and 1216, plus the half-precision 960 and 1216 that TensorRT
rem for RTX and DirectML run on the GPU. 704 and 1600 gain little (1600 is about 4 % faster than 1216 on
rem GPUs where it fits); "full" adds them and the other FP16 files.
cd /d "%~dp0\.."
if not exist target mkdir target
set NVIDIA=1
set FULL=
set FAILED=0
for %%A in (%*) do (
  if /i "%%~A"=="nonvidia" set NVIDIA=
  if /i "%%~A"=="full" set FULL=1
)

rem version from the workspace Cargo.toml: version = "x.y.z"
set VERSION=
for /f "tokens=2 delims==" %%V in ('findstr /b /c:"version = " Cargo.toml') do if not defined VERSION set "VERSION=%%~V"
set VERSION=%VERSION: =%
set VERSION=%VERSION:"=%
set NAME=Apsophos-%VERSION%-windows-x64
if not defined NVIDIA set NAME=%NAME%-directml
set OUT=dist\%NAME%
echo Packaging %NAME%...

echo [1/4] Checking the build...
set MISSING=
rem DirectML is built into the program; the shared providers DLL only loads
rem TensorRT for RTX, and a DirectML-only build does not make it
for %%F in (apsophos.exe DirectML.dll) do (
  if not exist target\release\%%F call set MISSING=%%MISSING%% %%F
)
if defined NVIDIA for %%F in (onnxruntime_providers_shared.dll onnxruntime_providers_nv_tensorrt_rtx.dll tensorrt_rtx_1_4.dll tensorrt_onnxparser_rtx_1_4.dll) do (
  if not exist target\release\%%F call set MISSING=%%MISSING%% %%F
)
if defined MISSING (
  call echo     Missing in target\release:%%MISSING%%
  echo     Run scripts\check.bat first ^(with TensorRT for RTX 1.4 extracted in C:\TensorRT-RTX-1.4...^),
  echo     or use "scripts\package.bat nonvidia" for a package without the NVIDIA DLLs.
  set FAILED=1
  goto end
)
if not exist models\scunet_color_real_448.onnx (
  echo     models\scunet_color_real_448.onnx is missing: see models\README.txt
  set FAILED=1
  goto end
)

echo [2/4] Copying the program, DLLs and models...
if exist "%OUT%" rd /s /q "%OUT%"
mkdir "%OUT%\models" "%OUT%\licenses"
copy /y target\release\apsophos.exe "%OUT%\" >nul
copy /y target\release\DirectML.dll "%OUT%\" >nul
if defined NVIDIA (
  copy /y target\release\onnxruntime_providers_shared.dll "%OUT%\" >nul
  copy /y target\release\onnxruntime_providers_nv_tensorrt_rtx.dll "%OUT%\" >nul
  copy /y target\release\tensorrt_rtx_1_4.dll "%OUT%\" >nul
  copy /y target\release\tensorrt_onnxparser_rtx_1_4.dll "%OUT%\" >nul
)
rem the FP16 files are for the GPU (TensorRT for RTX and DirectML)
if defined FULL (
  copy /y models\scunet_color_real_*.onnx "%OUT%\models\" >nul
) else (
  for %%M in (448 960 1216) do if exist models\scunet_color_real_%%M.onnx copy /y models\scunet_color_real_%%M.onnx "%OUT%\models\" >nul
  for %%M in (960 1216) do if exist models\scunet_color_real_%%M_fp16.onnx copy /y models\scunet_color_real_%%M_fp16.onnx "%OUT%\models\" >nul
)

echo [3/4] Adding the documents and licences...
powershell -NoProfile -Command "(Get-Content -Raw 'packaging\README.txt').Replace('@VERSION@', '%VERSION%') | Set-Content -NoNewline -Encoding UTF8 '%OUT%\README.txt'"
copy /y packaging\THIRD-PARTY-NOTICES.txt "%OUT%\" >nul
copy /y LICENSE "%OUT%\LICENSE.txt" >nul
copy /y LICENSE-EXCEPTION "%OUT%\LICENSE-EXCEPTION.txt" >nul
copy /y packaging\licenses\*.txt "%OUT%\licenses\" >nul

echo [4/4] Making the zip...
if exist "dist\%NAME%.zip" del /q "dist\%NAME%.zip"
tar -a -c -f "dist\%NAME%.zip" -C dist "%NAME%" 2>target\package.log
if errorlevel 1 (
  echo     tar failed ^(details in target\package.log^), trying PowerShell...
  powershell -NoProfile -Command "Compress-Archive -Path 'dist\%NAME%' -DestinationPath 'dist\%NAME%.zip'" >>target\package.log 2>&1
)

echo.
echo ===== Package =====
echo   folder: %CD%\%OUT%
powershell -NoProfile -Command "$s = (Get-ChildItem -Recurse -File '%OUT%' | Measure-Object Length -Sum).Sum; '  {0} files, {1:N0} MB' -f (Get-ChildItem -Recurse -File '%OUT%').Count, ($s / 1MB)"
for %%Z in ("dist\%NAME%.zip") do if exist "%%~fZ" (
  powershell -NoProfile -Command "'  zip: %%~fZ, {0:N0} MB' -f (%%~zZ / 1MB)"
) else (
  echo     the zip could not be made ^(details in target\package.log^)
  set FAILED=1
)
(echo %NAME% & dir /b /s "%OUT%") >> target\package.log

rem installer, when Inno Setup 6 is installed (ISCC on PATH or in its usual folder)
set ISCC=
for %%I in (ISCC.exe) do if not "%%~$PATH:I"=="" set "ISCC=%%~$PATH:I"
if not defined ISCC if exist "%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe" set "ISCC=%ProgramFiles(x86)%\Inno Setup 6\ISCC.exe"
if not defined ISCC if exist "%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe" set "ISCC=%LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe"
if defined ISCC (
  "%ISCC%" /Q /DAppVersion=%VERSION% "/DSourceDir=%CD%\%OUT%" "/DOutputBase=%NAME%-setup" packaging\apsophos.iss >>target\package.log 2>&1
  if errorlevel 1 (
    echo     the installer could not be made ^(details in target\package.log^)
    set FAILED=1
  ) else (
    echo   installer: %CD%\dist\%NAME%-setup.exe
  )
) else (
  echo   installer: skipped ^(Inno Setup 6 not installed^)
)

:end
echo.
pause
rem exit code 1 when nothing or only part was made, so a workflow stops here
exit /b %FAILED%

