# Signs files with the code-signing certificate in $env:SIGN_PFX (the .pfx in
# base64) and $env:SIGN_PASSWORD, with an RFC 3161 timestamp, so the
# signature stays valid after the certificate expires. Used by the release
# workflow when the repository has those secrets:
#   scripts\sign.ps1 target\release\apsophos.exe
param([Parameter(Mandatory, ValueFromRemainingArguments)] [string[]] $Files)
$ErrorActionPreference = 'Stop'

if (-not $env:SIGN_PFX) { throw 'SIGN_PFX is not set' }
$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Sort-Object FullName | Select-Object -Last 1
if (-not $signtool) { throw 'signtool.exe not found (Windows SDK)' }
$pfx = Join-Path ([System.IO.Path]::GetTempPath()) ("sign-" + [guid]::NewGuid() + ".pfx")
try {
    [System.IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:SIGN_PFX))
    foreach ($file in $Files) {
        & $signtool.FullName sign /f $pfx /p $env:SIGN_PASSWORD /fd sha256 /tr http://timestamp.digicert.com /td sha256 $file
        if ($LASTEXITCODE) { throw "signing $file failed" }
    }
} finally {
    Remove-Item -Force -ErrorAction SilentlyContinue $pfx
}
