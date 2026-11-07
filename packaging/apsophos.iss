; Installer of Apsophos (Inno Setup 6), built by scripts\package.bat when
; Inno Setup is installed and by the release workflow:
;   ISCC /DAppVersion=0.1.0 /DSourceDir=<full path of the package folder> packaging\apsophos.iss
; (paths in this script are relative to the packaging folder, hence the full
; path; the setup goes to dist\).
; It installs for the current user only (no administrator rights), in
; %LOCALAPPDATA%\Programs\Apsophos, with a Start menu entry and an
; uninstaller. The settings and caches in %APPDATA% and %LOCALAPPDATA%\Apsophos
; are left in place on uninstall.

#ifndef AppVersion
  #error Pass /DAppVersion=x.y.z
#endif
#ifndef SourceDir
  #error Pass /DSourceDir=<package folder>
#endif
#ifndef OutputBase
  #define OutputBase "Apsophos-" + AppVersion + "-windows-x64-setup"
#endif

[Setup]
; the identity of the program for Windows: never change it
AppId={{6F1C2A4E-8B37-4D5A-9E21-3C7B9D04A1F5}
AppName=Apsophos
AppVersion={#AppVersion}
AppVerName=Apsophos {#AppVersion}
AppPublisher=MR
AppCopyright=Copyright (C) 2026 MR
DefaultDirName={localappdata}\Programs\Apsophos
DefaultGroupName=Apsophos
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile={#SourceDir}\LICENSE.txt
OutputDir=..\dist
OutputBaseFilename={#OutputBase}
UninstallDisplayIcon={app}\apsophos.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "italian"; MessagesFile: "compiler:Languages\Italian.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Apsophos"; Filename: "{app}\apsophos.exe"
Name: "{group}\{cm:UninstallProgram,Apsophos}"; Filename: "{uninstallexe}"
Name: "{userdesktop}\Apsophos"; Filename: "{app}\apsophos.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\apsophos.exe"; Description: "{cm:LaunchProgram,Apsophos}"; Flags: nowait postinstall skipifsilent
