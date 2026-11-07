# Local checks on Windows: format, release build, tests, Clippy and, when
# installed, cargo-deny. A branch is merged into main only when all are green.
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

$logPath = Join-Path (Get-Location) 'target\check.log'
$sumPath = Join-Path (Get-Location) 'target\check-summary.txt'
$log = [System.IO.StreamWriter]::new($logPath, $false, [System.Text.UTF8Encoding]::new($false))
$log.AutoFlush = $true
$summary = [System.Collections.Generic.List[string]]::new()
$started = Get-Date
$env:RUST_BACKTRACE = '1'   # panics in tests show where they happened

function Say([string]$text, [string]$color = '') {
    if ($color) { Write-Host $text -ForegroundColor $color } else { Write-Host $text }
    $log.WriteLine($text)
}

function Note([string]$text) { $summary.Add($text) }

# First line of a short command's output, or why it could not run.
function Probe([string]$exe, [string[]]$arguments) {
    try {
        $lines = @(& $exe @arguments 2>&1 | ForEach-Object { "$_" } | Where-Object { $_ -and $_ -ne 'System.Management.Automation.RemoteException' })
        if ($lines.Count) { $lines[0] } else { '(no output)' }
    } catch { "not available ($($_.Exception.Message))" }
}

Say "check started $started" 'Cyan'
Note "check started $started"

# ---- Environment -----------------------------------------------------------
Note ''
Note '== environment'
$head = Probe 'git' @('log', '-1', '--format=%h %s')
$branch = Probe 'git' @('rev-parse', '--abbrev-ref', 'HEAD')
$dirty = @(& git status --porcelain 2>$null).Count
Note "git:      $branch @ $head ($dirty changed or untracked files)"
Note "rustc:    $(Probe 'rustc' @('-V'))"
Note "cargo:    $(Probe 'cargo' @('-V'))"
Note "windows:  $([System.Environment]::OSVersion.VersionString)"
try {
    $gpus = (Get-CimInstance Win32_VideoController | ForEach-Object { "$($_.Name) (driver $($_.DriverVersion))" }) -join '; '
    Note "gpu:      $gpus"
} catch { Note 'gpu:      unknown' }
$rtx = Find-RtxDir
Note "rtx:      $(if ($rtx) { $rtx } else { 'TensorRT for RTX 1.4 not found (DirectML only)' })"
$models = @(Get-ChildItem 'models' -Filter '*.onnx' -File -ErrorAction SilentlyContinue | ForEach-Object { $_.BaseName -replace '^scunet_color_real_', '' })
Note "models:   $(if ($models.Count) { $models -join ', ' } else { 'none in models\ (model tests are skipped; see models\README.txt)' })"
foreach ($line in $summary) { if ($line -and $line -notlike 'check started*') { Say $line 'DarkGray' } }

# ---- Steps -----------------------------------------------------------------
# Lines worth reading in the summary. Errors and warnings come with the
# `-->` line that says where they are.
$important = @(
    '^(error|warning)(\[[A-Za-z0-9_]+\])?:',
    '^\s+--> ',
    'panicked at',
    '^test .+ \.\.\. FAILED$',
    '^---- .+ stdout ----$',
    '^test result: ',
    '^Diff in ',
    '^error: could not compile',
    '^\s+Finished ',
    '^\s+(failed|rejected|advisories|bans|licenses|sources) '
)
$pattern = ($important | ForEach-Object { "($_)" }) -join '|'
$failures = 0

function Run-Step([string]$name, [string]$exe, [string[]]$arguments) {
    if ($Step -and $Step -ne $name) { return $true }
    $cmd = "$exe $($arguments -join ' ')"
    Say ''
    Say "===== $name : $cmd =====" 'Cyan'
    $watch = [System.Diagnostics.Stopwatch]::StartNew()
    $picked = [System.Collections.Generic.List[string]]::new()
    $warnings = 0
    & $exe @arguments 2>&1 | ForEach-Object {
        # cargo writes to stderr, which PowerShell wraps as error records; an
        # empty stderr line would otherwise print as "...RemoteException".
        $text = if ($_ -is [System.Management.Automation.ErrorRecord]) { "$($_.Exception.Message)" } else { "$_" }
        if ($text -eq 'System.Management.Automation.RemoteException') { $text = '' }
        $color = if ($text -match '^error') { 'Red' } elseif ($text -match '^warning') { 'Yellow' } elseif ($text -match 'FAILED|panicked') { 'Red' } else { '' }
        Say $text $color
        if ($text -match '^warning(\[[A-Za-z0-9_]+\])?:' -and $text -notmatch 'generated \d+ warning') { $warnings++ }
        if ($text -match $pattern) { $picked.Add($text) }
    }
    $code = $LASTEXITCODE
    $watch.Stop()
    $secs = [math]::Round($watch.Elapsed.TotalSeconds)
    $ok = $code -eq 0
    $status = if ($ok) { 'ok' } else { "FAILED (exit $code)"; $script:failures++ }
    Say "----- $name : $status in $secs s" $(if ($ok) { 'Green' } else { 'Red' })
    Note ''
    Note "== $name : $status in $secs s : $cmd"
    if ($warnings -gt 0) { Note "   $warnings warning(s)" }
    $max = 80
    foreach ($l in ($picked | Select-Object -First $max)) { Note "   $l" }
    if ($picked.Count -gt $max) { Note "   ... $($picked.Count - $max) more lines in target\check.log" }
    return $ok
}

# Everything in release, like the program the user runs: the tests with the
# real model are too slow in debug, and the GPU DLLs live in target\release.
Run-Step 'fmt' 'cargo' @('fmt', '--all', '--check') | Out-Null
$built = Run-Step 'build' 'cargo' @('build', '--release', '--workspace', '--all-targets')
if ($built -and (-not $Step -or $Step -eq 'build' -or $Step -eq 'test')) {
    Note ''
    Note '== dlls'
    foreach ($l in (Sync-Dlls)) { Say $l 'DarkGray'; Note "   $l" }
}
# the model tests share one GPU: one at a time
Run-Step 'test' 'cargo' @('test', '--workspace', '--release', '--no-fail-fast', '--', '--test-threads=1') | Out-Null
Run-Step 'clippy' 'cargo' @('clippy', '--workspace', '--all-targets', '--release', '--', '-D', 'warnings') | Out-Null
if (Get-Command 'cargo-deny' -ErrorAction SilentlyContinue) {
    Run-Step 'deny' 'cargo' @('deny', 'check') | Out-Null
} elseif (-not $Step -or $Step -eq 'deny') {
    Note ''
    Note '== deny : skipped (cargo install cargo-deny to enable it)'
}

$elapsed = [math]::Round(((Get-Date) - $started).TotalMinutes, 1)
$verdict = if ($failures -eq 0) { 'ALL GREEN' } else { "$failures step(s) FAILED" }
Note ''
Note "check finished $(Get-Date) : $verdict in $elapsed min"
$summary | Set-Content -Path $sumPath -Encoding UTF8
$log.Close()

Write-Host ''
Write-Host '================ summary ================' -ForegroundColor Cyan
Get-Content $sumPath | ForEach-Object { Write-Host $_ }
Write-Host ''
Write-Host "Full output: $logPath"
Write-Host "Summary:     $sumPath"
exit $failures
