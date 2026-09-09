; Chromazen Windows installer - Inno Setup 6.
; CI builds it with: iscc packaging/windows/Chromazen.iss /DAppVersion=<version from Cargo.toml>
; Signing is added later via SignTool=... once a certificate is available.

#ifndef AppVersion
#define AppVersion "0.1.3"
#endif

[Setup]
AppId={{057ca448-3cc3-4223-9ddf-713ab9bbf8e0}
AppName=Chromazen
AppVersion={#AppVersion}
DefaultDirName={autopf}\Chromazen
DefaultGroupName=Chromazen
DisableProgramGroupPage=yes
; Per-user install: no UAC prompt, installs under %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
OutputDir=..\..\dist
OutputBaseFilename=ChromazenSetup-{#AppVersion}-x64
SetupIconFile=..\..\assets\Chromazen.ico
UninstallDisplayIcon={app}\Chromazen.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
MinVersion=10.0
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\..\target\release\Chromazen.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Chromazen"; Filename: "{app}\Chromazen.exe"
Name: "{group}\Uninstall Chromazen"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Chromazen"; Filename: "{app}\Chromazen.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Chromazen.exe"; Description: "{cm:LaunchProgram,Chromazen}"; Flags: nowait postinstall skipifsilent
