; Aura Portfolio - Inno Setup script. Made by Akulav.
; One file installs the app the first time and updates it later:
;   * first run: asks for the install folder (the app AND its data - settings, portfolio cache, AI models,
;     the llama.cpp runtime, logs - go there, in {app}\data)
;   * later runs and in-app updates (/UPDATE): reuse the remembered folder without asking
; Build: iscc /DAppVersion=X.Y.Z /DExe=path\to\aura-portfolio.exe installer\aura-portfolio.iss
; The pictures and the icon come from assets/ (tools_dev/make_icons.py, from assets/icon.svg).

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\target\release\aura-portfolio.exe"
#endif

#define AppName "Aura Portfolio"
#define AppExe "aura-portfolio.exe"
#define SetupName "AuraPortfolio-setup"
; Marks the install folder (src/install.rs: MARKER).
#define Marker ".aura-app"

[Setup]
; NEVER change the AppId: it's how an upgrade and the uninstaller find this install.
AppId={{6208BE64-B4D0-4EDB-BF3A-510EE88F9FB5}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Akulav
AppPublisherURL=https://github.com/Akulav/t212-ai-dashboard
AppSupportURL=https://github.com/Akulav/t212-ai-dashboard/issues
AppUpdatesURL=https://github.com/Akulav/t212-ai-dashboard/releases
AppCopyright=Made by Akulav
VersionInfoVersion={#AppVersion}
VersionInfoCompany=Akulav
VersionInfoDescription={#AppName} Setup
VersionInfoProductName={#AppName}
; Per-user install: no administrator rights.
PrivilegesRequired=lowest
DefaultDirName={localappdata}\Aura Portfolio
; Ask for the folder on the first install only; updates reuse the previous one.
DisableDirPage=auto
UsePreviousAppDir=yes
DirExistsWarning=no
DisableProgramGroupPage=yes
DefaultGroupName={#AppName}
OutputBaseFilename={#SetupName}
OutputDir=..\dist
SetupIconFile=..\assets\icon.ico
WizardImageFile=..\assets\wizard-large.bmp,..\assets\wizard-large-200.bmp
WizardSmallImageFile=..\assets\wizard-small.bmp,..\assets\wizard-small-200.bmp
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
; (a running Aura Portfolio is closed - its local AI server ends with it - before its program is replaced)
CloseApplications=force
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
Compression=lzma2/max
SolidCompression=yes
LicenseFile=..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
english.LaunchApp=Launch Aura Portfolio
english.DataNote=The app and everything it keeps (settings, portfolio cache, AI models, logs) will be stored in this folder only.

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[InstallDelete]
; (an install made by the 0.2.x NSIS setup in this folder: its uninstaller and Start menu folder)
Type: files; Name: "{app}\uninstall.exe"
Type: filesandordirs; Name: "{autoprograms}\Aura Portfolio"
Type: files; Name: "{app}\aura-portfolio.exe.old"
Type: files; Name: "{app}\aura-portfolio.exe.new"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Comment: "Trading212 portfolio analytics with a local AI analyst - made by Akulav"
Name: "{autoprograms}\Uninstall {#AppName}"; Filename: "{uninstallexe}"; WorkingDir: "{app}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Comment: "Trading212 portfolio analytics with a local AI analyst - made by Akulav"; Tasks: desktopicon

[Run]
; Interactive install: the usual "Launch" checkbox.
Filename: "{app}\{#AppExe}"; WorkingDir: "{app}"; Description: "{cm:LaunchApp}"; Flags: nowait postinstall skipifsilent
; Silent in-app update (/UPDATE): start the new version again. It waits for the old one to let go of the data folder.
Filename: "{app}\{#AppExe}"; Parameters: "--after-update"; WorkingDir: "{app}"; Flags: nowait; Check: IsUpdate

[UninstallDelete]
Type: files; Name: "{app}\{#Marker}"
Type: files; Name: "{app}\aura-portfolio.exe.old"
Type: files; Name: "{app}\aura-portfolio.exe.new"
; The setup of an in-app update (kept in the app's own folder, never in %TEMP%).
Type: files; Name: "{app}\data\downloads\{#SetupName}.exe"

[Code]
function IsUpdate(): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/UPDATE') = 0 then
      Result := True;
end;

procedure InitializeWizard();
begin
  WizardForm.SelectDirLabel.Caption := WizardForm.SelectDirLabel.Caption + #13#10 + CustomMessage('DataNote');
end;

// One Apps entry only: this setup's. A 1.0 self-made install or the old 0.2.x NSIS setup registered itself as
// "Aura Portfolio"; that entry goes once this setup has installed.
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    SaveStringToFile(ExpandConstant('{app}\{#Marker}'), '{#AppVersion}', False);
    RegDeleteKeyIncludingSubkeys(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Aura Portfolio');
  end;
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
              'Also delete its data in ' + ExpandConstant('{app}') + ' ?' + #13#10 +
              'That deletes your settings, the cached portfolio history, insights, ' +
              'the downloaded AI models and the llama.cpp runtime for good.' + #13#10 +
              '(Your Trading212 API key is in the Windows Credential Manager, not in this folder.)' + #13#10#13#10 +
              'No keeps them: installing Aura Portfolio again brings everything back.',
              mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
    begin
      { Only what the app itself creates, never anything else kept in that folder. }
      DelTree(ExpandConstant('{app}\data'), True, True, True);
      DelTree(ExpandConstant('{app}\aura-data'), True, True, True);
    end;
    RemoveDir(ExpandConstant('{app}'));
  end;
end;
