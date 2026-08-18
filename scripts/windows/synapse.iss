#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceExe
  #error SourceExe must be defined
#endif
#ifndef OutputDir
  #define OutputDir "..\..\target\release\bundle\windows"
#endif
#ifndef SetupIcon
  #define SetupIcon "..\..\assets\branding\synapse-app-icon.ico"
#endif
#ifndef LicenseFile
  #define LicenseFile "..\..\LICENSE-MIT"
#endif

[Setup]
AppId={{8F0C1E3A-6B27-4D9A-9C41-7E5A2F8B4D10}
AppName=Synapse
AppVersion={#AppVersion}
AppVerName=Synapse {#AppVersion}
AppPublisher=xuyi
AppPublisherURL=https://github.com/08820048/Synapse
AppSupportURL=https://github.com/08820048/Synapse/issues
DefaultDirName={autopf}\Synapse
DefaultGroupName=Synapse
DisableProgramGroupPage=yes
LicenseFile={#LicenseFile}
OutputDir={#OutputDir}
OutputBaseFilename=Synapse-{#AppVersion}-windows-x64
SetupIconFile={#SetupIcon}
UninstallDisplayIcon={app}\Synapse.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
MinVersion=10.0
ChangesAssociations=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "Synapse.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\Synapse"; Filename: "{app}\Synapse.exe"
Name: "{autodesktop}\Synapse"; Filename: "{app}\Synapse.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Synapse.exe"; Description: "{cm:LaunchProgram,Synapse}"; Flags: nowait postinstall skipifsilent
