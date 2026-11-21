; Inno Setup 6 script for the Wisp Player installer.
;
;   cargo build --release
;   iscc /DExePath=target\release\wisp.exe installer\WispPlayer.iss
;
; writes dist\WispPlayer-Setup-<version>.exe. The installer works for the current user only (no
; administrator rights), adds a desktop shortcut (a task that is ticked by default) and a Start
; Menu entry, and registers an uninstaller under Settings > Apps.

#define AppName "Wisp Player"
#define AppVersion "0.1.0"
#define AppExe "wisp.exe"
#define AppUrl "https://github.com/DividedsWhimsicalVHS/WispPlayer"
; The built program; override with /DExePath=... when cargo's target directory is elsewhere.
#ifndef ExePath
  #define ExePath "..\target\release\wisp.exe"
#endif

[Setup]
; A fixed id lets a newer installer replace an older install in place.
AppId={{6E1B3C58-2F4D-4B7A-9C0E-D5A7F13E82B4}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Wisp Player contributors
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
; Installs for the current user only, so no administrator prompt ever appears; {autopf} is then
; %LOCALAPPDATA%\Programs.
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\assets\icon\wisp.ico
; The pictures come from assets\icon\build-icons.ps1, at 100% and 200% scale.
WizardImageFile=wizard-large.png,wizard-large-2x.png
WizardSmallImageFile=wizard-small.png,wizard-small-2x.png
DisableWelcomePage=no
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir=..\dist
OutputBaseFilename=WispPlayer-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
VersionInfoVersion={#AppVersion}
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\THIRD-PARTY-LICENSES.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
{ Wisp keeps its library, settings and playlists outside the install folder. Removing the program
  leaves them alone unless the user says otherwise (a silent uninstall never deletes them). }
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  DataDir: String;
begin
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
  begin
    DataDir := ExpandConstant('{localappdata}\Wisp');
    if DirExists(DataDir) then
      if MsgBox('Also delete your Wisp library, settings and playlists?' + #13#10 + #13#10 + DataDir,
                mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(DataDir, True, True, True);
  end;
end;
