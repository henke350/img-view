; Inno Setup script for the img-view Windows installer.
; Build it with installer\build.ps1, which builds the release .exe first and passes
; the version from Cargo.toml. The installer lands in target\installer\.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#define AppName "img-view"
#define AppExe "img-view.exe"

[Setup]
; Identifies the app to Windows across upgrades. Never change it.
AppId={{184016F7-A47D-4A22-B7BF-584FE28488DB}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=henke350
AppPublisherURL=https://github.com/henke350/img-view
; Installs for the current user without admin rights; an admin can choose "all users" instead.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\assets\eye.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir=..\target\installer
OutputBaseFilename=img-view-setup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; Tells Windows to refresh icons after the "Open with" entries change.
ChangesAssociations=yes

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked
Name: "openwith"; Description: "Add img-view to the ""Open with"" menu for images"; GroupDescription: "Windows integration:"

[Files]
Source: "..\target\release\{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; "Open with" support: registers the app and the image types it can open. It does not
; take over the default viewer; Windows asks the user for that.
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"; Tasks: openwith
Root: HKA; Subkey: "Software\Classes\Applications\{#AppExe}\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""; Tasks: openwith
#define Ext(E) 'Root: HKA; Subkey: "Software\Classes\Applications\' + AppExe + '\SupportedTypes"; ValueType: string; ValueName: "' + E + '"; ValueData: ""; Tasks: openwith' + NewLine + \
  'Root: HKA; Subkey: "Software\Classes\' + E + '\OpenWithList\' + AppExe + '"; Flags: uninsdeletekey; Tasks: openwith' + NewLine
{#Ext(".jpg")}
{#Ext(".jpeg")}
{#Ext(".png")}
{#Ext(".gif")}
{#Ext(".bmp")}
{#Ext(".tif")}
{#Ext(".tiff")}
{#Ext(".webp")}
{#Ext(".ico")}
{#Ext(".tga")}
{#Ext(".qoi")}
{#Ext(".pnm")}
{#Ext(".pbm")}
{#Ext(".pgm")}
{#Ext(".ppm")}
{#Ext(".hdr")}
{#Ext(".exr")}

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName} now"; Flags: nowait postinstall skipifsilent
