; Inno Setup 6 script for jimmy's Windows installer.
;
;   iscc /DAppVersion=0.1.0 /DSrcExe=C:\path\to\jimmy.exe packaging\windows\jimmy.iss
;
; Writes dist\jimmy-<AppVersion>-windows-x86_64-setup.exe (relative paths here are relative to
; this file). Installs for the current user, or for all users if the user picks that in the
; first dialog. Adds jimmy to "Open with" for common video files without taking over defaults.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef SrcExe
  #error Pass /DSrcExe=<path to jimmy.exe>
#endif

[Setup]
; Never change AppId: upgrades and the uninstaller find earlier installs by it.
AppId={{940F30DC-46EC-4F18-9287-F34C35D4659E}
AppName=jimmy
AppVersion={#AppVersion}
AppVerName=jimmy {#AppVersion}
AppPublisher=jimmy
DefaultDirName={autopf}\jimmy
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\..\LICENSE
SetupIconFile=..\..\assets\jimmy.ico
UninstallDisplayIcon={app}\jimmy.exe
UninstallDisplayName=jimmy
ChangesAssociations=yes
OutputDir=..\..\dist
OutputBaseFilename=jimmy-{#AppVersion}-windows-x86_64-setup
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SrcExe}"; DestDir: "{app}"; DestName: "jimmy.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\..\THIRD-PARTY.md"; DestDir: "{app}"; DestName: "THIRD-PARTY.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\jimmy"; Filename: "{app}\jimmy.exe"
Name: "{autodesktop}\jimmy"; Filename: "{app}\jimmy.exe"; Tasks: desktopicon

[Registry]
; HKA is HKLM for an all-users install and HKCU for a per-user one.
; Applications\jimmy.exe + SupportedTypes: offered in the "Open with" dialog for these types.
; <ext>\OpenWithList\jimmy.exe: listed in the "Open with" submenu. Neither changes the default app.
; Uninstall removes them again, and the <ext> keys too if nothing else is left in them.
Root: HKA; Subkey: "Software\Classes\Applications\jimmy.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "jimmy"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Applications\jimmy.exe\DefaultIcon"; ValueType: string; ValueData: "{app}\jimmy.exe,0"
Root: HKA; Subkey: "Software\Classes\Applications\jimmy.exe\shell\open\command"; ValueType: string; ValueData: """{app}\jimmy.exe"" ""%1"""
#define Exts "mkv mp4 m4v avi mov webm wmv flv mpg mpeg m2ts mts ts vob ogv 3gp divx asf f4v rmvb "
#define Rest
#define Ext
#sub AddExt
  #expr Ext = Copy(Rest, 1, Pos(" ", Rest) - 1)
  #expr Rest = Copy(Rest, Pos(" ", Rest) + 1)
Root: HKA; Subkey: "Software\Classes\Applications\jimmy.exe\SupportedTypes"; ValueType: string; ValueName: ".{#Ext}"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\.{#Ext}"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.{#Ext}\OpenWithList"; Flags: uninsdeletekeyifempty
Root: HKA; Subkey: "Software\Classes\.{#Ext}\OpenWithList\jimmy.exe"; Flags: uninsdeletekey
#endsub
#for {Rest = Exts; Len(Rest) > 0; 0} AddExt

[Run]
Filename: "{app}\jimmy.exe"; Description: "{cm:LaunchProgram,jimmy}"; Flags: nowait postinstall skipifsilent
