# Writes the three winget manifest files of a release (version, installer,
# English locale), to submit to the winget-pkgs repository by hand:
#   scripts\winget-manifest.ps1 -Version 0.1.0 -Installer dist\Apsophos-0.1.0-windows-x64-directml-setup.exe `
#       -InstallerUrl https://github.com/<owner>/<repo>/releases/download/v0.1.0/<file> -OutDir dist\winget
# The release workflow runs it with the URL of the release asset.
param(
    [Parameter(Mandatory)] [string] $Version,
    [Parameter(Mandatory)] [string] $Installer,
    [Parameter(Mandatory)] [string] $InstallerUrl,
    [Parameter(Mandatory)] [string] $OutDir
)
$ErrorActionPreference = 'Stop'

$id = 'MR.Apsophos'
$manifestVersion = '1.6.0'
$sha256 = (Get-FileHash -Algorithm SHA256 $Installer).Hash
New-Item -ItemType Directory -Force $OutDir | Out-Null

$files = @{
    "$id.yaml" = @"
PackageIdentifier: $id
PackageVersion: $Version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $manifestVersion
"@
    "$id.installer.yaml" = @"
PackageIdentifier: $id
PackageVersion: $Version
InstallerType: inno
Scope: user
InstallModes:
  - interactive
  - silent
Installers:
  - Architecture: x64
    InstallerUrl: $InstallerUrl
    InstallerSha256: $sha256
ManifestType: installer
ManifestVersion: $manifestVersion
"@
    "$id.locale.en-US.yaml" = @"
PackageIdentifier: $id
PackageVersion: $Version
PackageLocale: en-US
Publisher: MR
PackageName: Apsophos
License: GPL-3.0-only
Copyright: Copyright (C) 2026 MR
ShortDescription: Photo noise reduction with the SCUNet neural network, on the GPU.
Tags:
  - photo
  - denoise
  - noise-reduction
ManifestType: defaultLocale
ManifestVersion: $manifestVersion
"@
}
foreach ($name in $files.Keys) {
    # UTF-8 without BOM, as winget expects
    [System.IO.File]::WriteAllText((Join-Path $OutDir $name), $files[$name] + "`n", (New-Object System.Text.UTF8Encoding $false))
}
Write-Host "winget manifest for $id $Version in $OutDir (installer SHA-256 $sha256)"
