; Aura Portfolio's Windows setup (Inno Setup 6): the full wizard - Welcome, License, Folder, Shortcuts, Install,
; Finish (with "Launch Aura Portfolio") - per user, no administrator rights, and its uninstaller (Settings -> Apps and
; the Start menu). The release workflow builds it:
;
;   iscc /DAppVersion=1.0.0 /DExe=<path>\aura-portfolio.exe /O<out dir> installer\aura-portfolio.iss
;
; It installs only the program. Its data (settings, portfolio cache, models, the llama.cpp runtime) lives in the
; data folder next to it, {app}\data, and survives upgrades. The program keeps itself up to date afterwards
; (src/updater.rs), and src/install.rs keeps this setup's Apps entry at the version an update brought.
; The pictures and the icon come from assets/ (tools_dev/make_icons.py, from assets/icon.svg).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\target\release\aura-portfolio.exe"
#endif

[Setup]
; NEVER change the AppId: it's how an upgrade and the uninstaller find this install. src/install.rs has it too
; (the Apps entry "{AppId}_is1" whose version it keeps current).
AppId={{6208BE64-B4D0-4EDB-BF3A-510EE88F9FB5}
AppName=Aura Portfolio
AppVersion={#AppVersion}
AppVerName=Aura Portfolio {#AppVersion}
AppPublisher=Akulav
AppPublisherURL=https://github.com/Akulav/t212-ai-dashboard
AppSupportURL=https://github.com/Akulav/t212-ai-dashboard/issues
AppUpdatesURL=https://github.com/Akulav/t212-ai-dashboard/releases
VersionInfoVersion={#AppVersion}
VersionInfoDescription=Aura Portfolio Setup
VersionInfoProductName=Aura Portfolio
DefaultDirName={localappdata}\Aura Portfolio
; (Inno Setup hides the Welcome and folder pages unless told: this setup shows both)
DisableWelcomePage=no
DisableDirPage=no
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
DirExistsWarning=no
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
OutputBaseFilename=Aura-Portfolio-Setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\aura-portfolio.exe
UninstallDisplayName=Aura Portfolio
WizardStyle=modern
WizardImageFile=..\assets\wizard-large.bmp,..\assets\wizard-large-200.bmp
WizardSmallImageFile=..\assets\wizard-small.bmp,..\assets\wizard-small-200.bmp
Compression=lzma2/max
SolidCompression=yes
; (a running Aura Portfolio is closed - its local AI server ends with it - before its program is replaced)
CloseApplications=force
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[InstallDelete]
; (an install made by the 0.2.x NSIS setup in this folder: its uninstaller and Start menu folder)
Type: files; Name: "{app}\uninstall.exe"
Type: filesandordirs; Name: "{autoprograms}\Aura Portfolio"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "aura-portfolio.exe"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Aura Portfolio"; Filename: "{app}\aura-portfolio.exe"; WorkingDir: "{app}"; Comment: "Trading212 portfolio analytics with a local AI analyst"
Name: "{autoprograms}\Uninstall Aura Portfolio"; Filename: "{uninstallexe}"; WorkingDir: "{app}"
Name: "{autodesktop}\Aura Portfolio"; Filename: "{app}\aura-portfolio.exe"; WorkingDir: "{app}"; Comment: "Trading212 portfolio analytics with a local AI analyst"; Tasks: desktopicon

[Run]
Filename: "{app}\aura-portfolio.exe"; Description: "{cm:LaunchProgram,Aura Portfolio}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; (what the program left beside itself after an update)
Type: files; Name: "{app}\aura-portfolio.exe.old"
Type: files; Name: "{app}\aura-portfolio.exe.new"

[Code]
// One Apps entry only: this setup's. A self-made install (src/install.rs) or the old 0.2.x NSIS setup registered
// itself as "Aura Portfolio"; that entry goes once this setup has installed.
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Aura Portfolio');
end;

// The uninstaller removes the program and its shortcuts; your data stays unless you say so.
// A running Aura Portfolio from this folder is ended first (Windows ends its local AI server with it).
procedure StopRunning();
var
  Code: Integer;
begin
  Exec('powershell.exe',
       '-NoProfile -NonInteractive -Command "Get-Process aura-portfolio -ErrorAction SilentlyContinue | ' +
       'Where-Object { $_.Path -eq ''' + ExpandConstant('{app}') + '\aura-portfolio.exe'' } | Stop-Process -Force"',
       '', SW_HIDE, ewWaitUntilTerminated, Code);
  Sleep(1500);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    StopRunning();
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
  begin
    if MsgBox('Aura Portfolio is uninstalled.' + #13#10#13#10 +
              'Also delete its folder ' + ExpandConstant('{app}') + ' ?' + #13#10 +
              'That deletes your settings, the cached portfolio history, insights, reports index, ' +
              'the downloaded AI models and the llama.cpp runtime for good.' + #13#10 +
              '(Your Trading212 API key is in the Windows Credential Manager, not in this folder.)' + #13#10#13#10 +
              'No keeps them: installing Aura Portfolio again brings everything back.',
              mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
      DelTree(ExpandConstant('{app}'), True, True, True);
  end;
end;
