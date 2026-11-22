; GitFleet Windows Installer Script (Inno Setup 6.3+)
;
; Build manually on Windows:
;   iscc /DMyAppVersion=0.6.0 /DMyExePath=..\..\target\release\gitfleet.exe ^
;        /DMyArch=x64compatible /DMyOutputDir=..\..\dist\out ^
;        /DMyOutputName=gitfleet-windows-x86_64-setup packaging\windows\gitfleet.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.6.0"
#endif
#ifndef MyExePath
  #define MyExePath "..\..\target\release\gitfleet.exe"
#endif
#ifndef MyArch
  #define MyArch "x64compatible"
#endif
#ifndef MyOutputDir
  #define MyOutputDir "..\..\dist\out"
#endif
#ifndef MyOutputName
  #define MyOutputName "gitfleet-windows-x86_64-setup"
#endif

[Setup]
AppId={{B7D4C3E2-6F9A-4E8B-9A2D-1C5E8F4A7B30}
AppName=GitFleet
AppVersion={#MyAppVersion}
AppVerName=GitFleet {#MyAppVersion}
AppPublisher=IOZ Labs
AppPublisherURL=https://github.com/ioz-labs/gitfleet
AppSupportURL=https://github.com/ioz-labs/gitfleet/issues
AppUpdatesURL=https://github.com/ioz-labs/gitfleet/releases
DefaultDirName={autopf}\GitFleet
DefaultGroupName=GitFleet
DisableProgramGroupPage=yes
LicenseFile=..\..\LICENSE
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\gitfleet.exe
UninstallDisplayName=GitFleet
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed={#MyArch}
ArchitecturesInstallIn64BitMode={#MyArch}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
OutputDir={#MyOutputDir}
OutputBaseFilename={#MyOutputName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#MyExePath}"; DestDir: "{app}"; DestName: "gitfleet.exe"; Flags: ignoreversion
Source: "..\..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\GitFleet"; Filename: "{app}\gitfleet.exe"
Name: "{autodesktop}\GitFleet"; Filename: "{app}\gitfleet.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\gitfleet.exe"; Description: "{cm:LaunchProgram,GitFleet}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\gitfleet.exe"; Flags: nowait; Check: ShouldLaunchSilent

[Code]
function ShouldLaunchSilent: Boolean;
begin
  Result := WizardSilent and (ExpandConstant('{param:relaunch|0}') = '1');
end;
