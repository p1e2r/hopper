; Inno Setup script for the Windows installer.
; Built in CI: ISCC /DAppVersion=<version> installer\hopper.iss
; Installs per user (no admin prompt) into %LOCALAPPDATA%\Programs\Hopper, so the
; in-app updater can replace Hopper.exe and Windows search finds the Start menu entry.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{5F85A390-7EA5-4145-8916-858002549B86}
AppName=Hopper
AppVersion={#AppVersion}
AppVerName=Hopper {#AppVersion}
AppPublisher=p1e2r
AppPublisherURL=https://p1e2r.github.io/hopper/
AppSupportURL=https://github.com/p1e2r/hopper/issues
AppUpdatesURL=https://p1e2r.github.io/hopper/
DefaultDirName={autopf}\Hopper
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\dist
OutputBaseFilename=Hopper-windows-x64-setup
SetupIconFile=..\assets\icon\icon.ico
UninstallDisplayIcon={app}\Hopper.exe
UninstallDisplayName=Hopper
LicenseFile=..\LICENSE
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\x86_64-pc-windows-msvc\release\hopper.exe"; DestDir: "{app}"; DestName: "Hopper.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Hopper"; Filename: "{app}\Hopper.exe"; Comment: "Minecraft mod manager"
Name: "{autodesktop}\Hopper"; Filename: "{app}\Hopper.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Hopper.exe"; Description: "{cm:LaunchProgram,Hopper}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Leftover from an in-app update on Windows.
Type: files; Name: "{app}\Hopper.exe.old"
