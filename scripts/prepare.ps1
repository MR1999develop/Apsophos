# One-off preparation on Windows, before scripts\check.bat:
#   - the half-precision (FP16) models that TensorRT for RTX and DirectML use, made with
#     Python + NumPy from the normal models in models\ when missing;
#   - where TensorRT for RTX 1.4 was found;
#   - with -RepairDlls, ONNX Runtime's DLLs fetched again: a build left stale
#     or missing DLLs in target\release (the GPU then fails with 887A0004).
# Output also in target\prepare.log.
#
#   scripts\prepare.bat
#   scripts\prepare.bat -RepairDlls
param([switch]$RepairDlls)
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot)
. "$PSScriptRoot\common.ps1"
New-Item -ItemType Directory -Force target | Out-Null
Start-Transcript -Path 'target\prepare.log' -Force | Out-Null
$failures = 0

function Step([string]$text) { Write-Host ''; Write-Host "===== $text" -ForegroundColor Cyan }
function Fail([string]$text) { Write-Host $text -ForegroundColor Red; $script:failures++ }

Step 'Models'
$tiles = 448, 704, 960, 1216, 1600
if (-not (Test-Path 'models\scunet_color_real_448.onnx')) {
    Fail 'models\scunet_color_real_448.onnx is missing: see models\README.txt to generate it'
}
$needFp16 = @($tiles | Where-Object {
        (Test-Path "models\scunet_color_real_$_.onnx") -and -not (Test-Path "models\scunet_color_real_${_}_fp16.onnx")
    })
if ($needFp16.Count -gt 0) {
    Write-Host "Making the FP16 models for $($needFp16 -join ', ') px with Python..."
    $sources = $needFp16 | ForEach-Object { "models\scunet_color_real_$_.onnx" }
    & python tools\make_fp16_model.py @sources
    if ($LASTEXITCODE -ne 0) { Fail 'Python or NumPy not available: TensorRT for RTX will use the FP32 models for those sizes' }
}
Get-ChildItem 'models' -Filter '*.onnx' -File -ErrorAction SilentlyContinue | ForEach-Object {
    $hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower()
    Write-Host ("{0,-38} {1,6:N1} MB  {2}" -f $_.Name, ($_.Length / 1MB), $hash)
}

Step 'TensorRT for RTX 1.4'
$rtx = Find-RtxDir
if ($rtx) {
    Write-Host "found in $rtx"
} else {
    Write-Host 'not found: the program uses DirectML (about 4 times slower on RTX cards).'
    Write-Host 'Extract the NVIDIA TensorRT for RTX 1.4 zip to C:\TensorRT-RTX-1.4... and run this again.'
}

if ($RepairDlls) {
    Step 'ONNX Runtime DLLs'
    # cargo does not replace DLLs left by an earlier build: remove them and
    # rebuild ort-sys, which copies the current ones
    Remove-Item "$ReleaseDir\*.dll", "$ReleaseDir\deps\*.dll" -ErrorAction SilentlyContinue
    & cargo clean -p ort-sys --release
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail 'build failed: see the messages above' }
    foreach ($l in (Sync-Dlls)) { Write-Host $l }
    $missing = @($OrtDlls | Where-Object { -not (Test-Path (Join-Path $ReleaseDir $_)) })
    if ($missing.Count -gt 0) {
        Fail "still missing: $($missing -join ', '). Delete $env:LOCALAPPDATA\ort.pyke.io (ONNX Runtime's download cache) and run this again."
    }
}

Step 'Versions'
& rustup --version
& cargo --version
& rustc --version

Write-Host ''
if ($failures -eq 0) { Write-Host 'Preparation done.' -ForegroundColor Green } else { Write-Host "$failures problem(s): see above." -ForegroundColor Red }
Stop-Transcript | Out-Null
exit $failures
