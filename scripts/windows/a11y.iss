; Inno Setup script for the a11y Windows installer.
; Built by scripts/package-windows-installer.sh (CI: .github/workflows/release.yml).
;
;   iscc /DAppVersion=0.1.0-alpha.9 /DNumericVersion=0.1.0.9 /DBinDir=<dir with a11y.exe> scripts\windows\a11y.iss
;
; Output: dist\a11y-<AppVersion>-x86_64-windows-setup.exe

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef NumericVersion
  #define NumericVersion "0.0.0.0"
#endif
#ifndef BinDir
  #define BinDir "target\x86_64-pc-windows-msvc\release"
#endif

#define AppName "A11y"
#define AppExe "a11y.exe"

[Setup]
; Stable GUID: never change it, upgrades install over the previous version through it.
AppId={{0B737168-AB26-4B05-8DC8-C50F8F8D3E13}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=debajay3
AppPublisherURL=https://github.com/debajay3/A11y
AppSupportURL=https://github.com/debajay3/A11y/issues
AppUpdatesURL=https://github.com/debajay3/A11y/releases
VersionInfoVersion={#NumericVersion}
VersionInfoProductVersion={#NumericVersion}
VersionInfoProductName={#AppName}
VersionInfoDescription={#AppName} setup
VersionInfoCompany=debajay3
; Per-user by default (no UAC prompt); the wizard offers "all users" via a dialog.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Paths below are relative to the repository root.
SourceDir=..\..
OutputDir=dist
OutputBaseFilename=a11y-{#AppVersion}-x86_64-windows-setup
SetupIconFile=assets\icon\a11y.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
LicenseFile=LICENSE-MIT
Compression=lzma2/ultra
SolidCompression=yes
WizardStyle=modern
ChangesAssociations=yes
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "fileassoc"; Description: "Open .a11y files with {#AppName}"; GroupDescription: "File associations:"

[Files]
Source: "{#BinDir}\a11y.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\a11y-mcp.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\a11y-diff.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-MIT"; DestDir: "{app}"; Flags: ignoreversion
Source: "LICENSE-APACHE"; DestDir: "{app}"; Flags: ignoreversion
Source: "README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "crates\a11y-fonts\assets\inter\OFL.txt"; DestDir: "{app}\licenses"; DestName: "Inter-OFL.txt"; Flags: ignoreversion
Source: "crates\a11y-app\assets\icons\lucide\LICENSE"; DestDir: "{app}\licenses"; DestName: "Lucide-LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; HKA is HKCU for a per-user install and HKLM for an all-users install.
Root: HKA; Subkey: "Software\Classes\.a11y"; ValueType: string; ValueName: ""; ValueData: "a11y.Document"; Flags: uninsdeletevalue; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\.a11y\OpenWithProgids"; ValueType: string; ValueName: "a11y.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\a11y.Document"; ValueType: string; ValueName: ""; ValueData: "a11y design"; Flags: uninsdeletekey; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\a11y.Document\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"; Tasks: fileassoc
Root: HKA; Subkey: "Software\Classes\a11y.Document\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: fileassoc

; The a11y:// link scheme: clicking a session link opens the Join dialog.
Root: HKA; Subkey: "Software\Classes\a11y"; ValueType: string; ValueName: ""; ValueData: "URL:a11y session link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\a11y"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\a11y\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\{#AppExe},0"
Root: HKA; Subkey: "Software\Classes\a11y\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
; In-app update ("Restart to update" runs this setup with /SILENT /RELAUNCH=1): postinstall entries do not run
; silently, so start the new version here instead. A plain silent install (/VERYSILENT /CURRENTUSER) does not.
Filename: "{app}\{#AppExe}"; Flags: nowait skipifnotsilent; Check: Relaunching

[Code]
// True when Setup was started with /RELAUNCH=1, which only the updater inside a11y passes.
function Relaunching: Boolean;
begin
  Result := ExpandConstant('{param:RELAUNCH|0}') = '1';
end;
