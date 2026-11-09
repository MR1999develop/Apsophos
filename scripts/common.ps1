# Helpers shared by the Windows scripts (dot-sourced: . "$PSScriptRoot\common.ps1").
# Paths are relative to the repository root, where every script starts.

# DLLs of TensorRT for RTX 1.4, the version the ONNX Runtime build expects.
$RtxDlls = @('tensorrt_rtx_1_4.dll', 'tensorrt_onnxparser_rtx_1_4.dll')
# DLLs that ONNX Runtime's build copies next to the program; without them the
# GPU cannot work (Windows' own old DirectML answers 887A0004).
$OrtDlls = @('DirectML.dll', 'onnxruntime_providers_shared.dll', 'onnxruntime_providers_nv_tensorrt_rtx.dll')
# Cargo's target directory: CARGO_TARGET_DIR when set, as for cargo itself.
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$ReleaseDir = Join-Path $TargetDir 'release'

# The dependencies' sources sit under the user's folder and the files their
# build scripts generate under this folder; both paths end up in the program
# (panic messages): write neutral prefixes instead, so the package carries no
# user name or local folder. These flags go in the Windows target's variable,
# which Cargo joins with the flags of .cargo/config.toml (Control Flow Guard,
# CET); the variable splits at spaces, so a path with a space is not remapped
# and a warning says so. Every script that builds sets the same flags, so
# they share one build.
$cargoHome = if ($env:CARGO_HOME) { $env:CARGO_HOME } else { Join-Path $env:USERPROFILE '.cargo' }
$remap = @()
foreach ($pair in @(
        @((Join-Path $cargoHome 'registry\src'), 'crates'),
        @((Get-Location).Path, 'apsophos'))) {
    if ($pair[0] -match '\s') {
        Write-Warning "not remapped (the path has a space): $($pair[0])"
    } else {
        $remap += "--remap-path-prefix=$($pair[0])=$($pair[1])"
    }
}
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = $remap -join ' '

# Folder holding the TensorRT for RTX DLLs: PATH first, then the usual places
# where the NVIDIA zip is extracted. $null if not found.
function Find-RtxDir {
    foreach ($dir in ($env:Path -split ';' | Where-Object { $_ })) {
        if (Test-Path (Join-Path $dir $RtxDlls[0]) -PathType Leaf) { return $dir }
    }
    $roots = @('C:\', "$env:ProgramFiles\NVIDIA", $env:USERPROFILE, (Join-Path $env:USERPROFILE 'Downloads'))
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        foreach ($d in (Get-ChildItem $root -Directory -Filter 'TensorRT-RTX-1.4*' -ErrorAction SilentlyContinue)) {
            foreach ($sub in @('bin', 'lib')) {
                $candidate = Join-Path $d.FullName $sub
                if (Test-Path (Join-Path $candidate $RtxDlls[0]) -PathType Leaf) { return $candidate }
            }
        }
    }
    return $null
}

# Copies the TensorRT for RTX DLLs into each folder of $Dirs (made when
# missing). check.ps1 puts them next to the program of both build profiles
# before `cargo xtask check`, which hands every DLL there to the test
# executables. Returns the line to report.
function Copy-RtxDlls([string[]]$Dirs) {
    $rtx = Find-RtxDir
    if (-not $rtx) {
        return 'TensorRT for RTX 1.4 not found: the program will use DirectML (extract the NVIDIA zip to C:\TensorRT-RTX-1.4...)'
    }
    foreach ($dir in $Dirs) {
        New-Item -ItemType Directory -Force $dir | Out-Null
        foreach ($dll in $RtxDlls) { Copy-Item (Join-Path $rtx $dll) $dir -Force }
    }
    return "TensorRT for RTX 1.4 from $rtx"
}

# ONNX Runtime's DLLs missing next to the program of the release build.
function Find-MissingOrtDlls {
    return @($OrtDlls | Where-Object { -not (Test-Path (Join-Path $ReleaseDir $_)) })
}

# Puts the TensorRT for RTX DLLs next to the program and gives the test
# executables (in deps\) the same DLLs as the program. Returns the lines to
# report: what was copied and what is missing.
function Sync-Dlls {
    $report = [System.Collections.Generic.List[string]]::new()
    if (-not (Test-Path $ReleaseDir)) { $report.Add("no $ReleaseDir yet: build first"); return $report }
    $report.Add((Copy-RtxDlls @($ReleaseDir)))
    $missing = Find-MissingOrtDlls
    if ($missing.Count -gt 0) {
        $report.Add("MISSING next to the program: $($missing -join ', ') - run scripts\prepare.bat -RepairDlls")
    }
    $deps = Join-Path $ReleaseDir 'deps'
    if (Test-Path $deps) { Copy-Item (Join-Path $ReleaseDir '*.dll') $deps -Force }
    return $report
}
