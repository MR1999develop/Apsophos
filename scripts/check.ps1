# Local checks on Windows. The checks themselves are `cargo xtask check`
# (format, build and tests in release and in debug, Clippy with warnings as
# errors, cargo-deny), the same command the CI runs on Windows and Ubuntu.
# This script adds what only this computer has: the TensorRT for RTX DLLs
# next to the program, and the Windows version, GPUs and TensorRT for RTX in
# the summary. A branch is merged into main only when all are green.
#
# Everything is shown on screen while it runs and written to target\check.log.
# target\check-summary.txt gets, for each step, its status and time and the
# lines that matter (errors, warnings with their location, failed tests,
# panics, format differences), plus the environment the checks ran in.
# The summary is what Claude reads to fix problems.
#
#   scripts\check.bat               all steps
#   scripts\check.bat -Step test    only one step (fmt, build, test, clippy, deny)
param(
    [ValidateSet('', 'fmt', 'build', 'test', 'clippy', 'deny')][string]$Step = ''
)
$ErrorActionPreference = 'Continue'
Set-Location (Split-Path $PSScriptRoot)
. "$PSScriptRoot\common.ps1"
New-Item -ItemType Directory -Force target | Out-Null

$sumPath = Join-Path (Get-Location) 'target\check-summary.txt'
$notesPath = Join-Path (Get-Location) 'target\check-notes.txt'
$builds = (-not $Step) -or ($Step -eq 'build') -or ($Step -eq 'test')
# a summary from an earlier run must not pass for this one
Remove-Item $sumPath -ErrorAction SilentlyContinue

# ---- What only this computer knows, for the summary's environment ---------
$notes = [System.Collections.Generic.List[string]]::new()
$notes.Add("started:  $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')")
$notes.Add("windows:  $([System.Environment]::OSVersion.VersionString)")
try {
    $gpus = (Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name) (driver $($_.DriverVersion))" }) -join '; '
    $notes.Add("gpu:      $gpus")
} catch { $notes.Add('gpu:      unknown') }
if ($builds) {
    # next to the program of both profiles before the build: xtask then puts
    # every DLL there into deps\, where the test executables are
    $notes.Add("rtx:      $(Copy-RtxDlls @((Join-Path $TargetDir 'release'), (Join-Path $TargetDir 'debug')))")
} else {
    $rtx = Find-RtxDir
    $notes.Add("rtx:      $(if ($rtx) { $rtx } else { 'TensorRT for RTX 1.4 not found (DirectML only)' })")
}
# UTF-8 without a byte order mark, which xtask would read as text
[System.IO.File]::WriteAllLines($notesPath, $notes, [System.Text.UTF8Encoding]::new($false))

# ---- The checks ---------------------------------------------------------------
$xtaskArgs = @('xtask', 'check', '--notes', $notesPath)
if ($Step) { $xtaskArgs += @('--step', $Step) }
& cargo @xtaskArgs 2>&1 | ForEach-Object {
    # cargo writes to stderr, which PowerShell wraps as error records; an
    # empty stderr line would otherwise print as "...RemoteException".
    $text = if ($_ -is [System.Management.Automation.ErrorRecord]) { "$($_.Exception.Message)" } else { "$_" }
    if ($text -eq 'System.Management.Automation.RemoteException') { $text = '' }
    $color = if ($text -match '^error') { 'Red' }
    elseif ($text -match '^warning') { 'Yellow' }
    elseif ($text -match 'FAILED|panicked') { 'Red' }
    elseif ($text -match '^-----.* : ok in ') { 'Green' }
    elseif ($text -match '^=====') { 'Cyan' }
    else { '' }
    if ($color) { Write-Host $text -ForegroundColor $color } else { Write-Host $text }
}
$failures = $LASTEXITCODE

$utf8 = [System.Text.UTF8Encoding]::new($false)
if (-not (Test-Path $sumPath)) {
    # xtask itself did not build or run
    $line = "cargo xtask check did not run (exit $failures): see the output above"
    Write-Host $line -ForegroundColor Red
    [System.IO.File]::WriteAllText($sumPath, "$line`n", $utf8)
    if ($failures -eq 0) { $failures = 1 }
} elseif ($builds) {
    $missing = Find-MissingOrtDlls
    if ($missing.Count -gt 0) {
        $line = "MISSING next to the program: $($missing -join ', ') - run scripts\prepare.bat -RepairDlls"
        Write-Host $line -ForegroundColor Red
        [System.IO.File]::AppendAllText($sumPath, "`n== dlls`n   $line`n", $utf8)
    }
}
exit $failures
