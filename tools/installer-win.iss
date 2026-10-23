; The Windows installer of Shot, compiled by tools/installer-win.py with Inno Setup 6.
;
; Per user, no administrator rights: Shot goes to %LOCALAPPDATA%\Programs\Shot, the folder the
; 1.0 README suggested for the portable shot.exe, so an update over it keeps start at login.
; The installer adds a Start menu shortcut and an entry in Settings > Apps; it never touches the
; settings in %APPDATA%\Shot or the shots in Pictures\Shot, and neither does the uninstaller.

#ifndef Version
  #error Pass /DVersion=x.y.z (tools/installer-win.py does)
#endif
#ifndef Exe
  #define Exe "..\target\release\shot.exe"
#endif
#ifndef Icon
  #error Pass /DIcon=<path to shot.ico> (tools/installer-win.py writes it)
#endif

[Setup]
; Never change it: Windows finds the installed Shot by it.
AppId={{486D4483-1143-487B-B437-BE91779EE346}
AppName=Shot
AppVersion={#Version}
AppVerName=Shot {#Version}
AppPublisher=Nikolay Pozharskiy
AppPublisherURL=https://github.com/poznik/shot
AppSupportURL=https://github.com/poznik/shot/issues
AppUpdatesURL=https://github.com/poznik/shot/releases
VersionInfoVersion={#Version}
VersionInfoDescription=Shot installer
PrivilegesRequired=lowest
DefaultDirName={autopf}\Shot
DisableDirPage=auto
DisableProgramGroupPage=yes
DisableReadyPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 22H2 (SPEC §9).
MinVersion=10.0.19045
SetupIconFile={#Icon}
UninstallDisplayIcon={app}\shot.exe
UninstallDisplayName=Shot
WizardStyle=modern
ShowLanguageDialog=no
LanguageDetectionMethod=uilanguage
Compression=lzma2/max
SolidCompression=yes
OutputBaseFilename=Shot-{#Version}-windows-x64-setup
; [Code] ends Shot before the files are copied; this is only the fallback.
CloseApplications=force
RestartApplications=no

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Shot"; Filename: "{app}\shot.exe"

[Run]
Filename: "{app}\shot.exe"; Description: "{cm:LaunchProgram,Shot}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; The log; the settings in %APPDATA%\Shot stay for a later install.
Type: filesandordirs; Name: "{localappdata}\Shot"

[Code]
const
  RunKey = 'Software\Microsoft\Windows\CurrentVersion\Run';
  ApprovedKey = 'Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run';
  RunValue = 'Shot';
  PROCESS_TERMINATE = $0001;
  SYNCHRONIZE = $00100000;

function GetWindowThreadProcessId(Wnd: HWND; var ProcessId: Cardinal): Cardinal;
  external 'GetWindowThreadProcessId@user32.dll stdcall';
function OpenProcess(Access: Cardinal; Inherit: Boolean; ProcessId: Cardinal): THandle;
  external 'OpenProcess@kernel32.dll stdcall';
function TerminateProcess(Process: THandle; ExitCode: Cardinal): Boolean;
  external 'TerminateProcess@kernel32.dll stdcall';
function WaitForSingleObject(Handle: THandle; Milliseconds: Cardinal): Cardinal;
  external 'WaitForSingleObject@kernel32.dll stdcall';
function CloseHandle(Handle: THandle): Boolean;
  external 'CloseHandle@kernel32.dll stdcall';

{ Ends the running Shot, wherever its shot.exe lies: it has one copy per user, so a Shot left
  running would take the new one's start. Its hidden tray window gives it away. }
procedure EndShot;
var
  Wnd: HWND;
  Pid: Cardinal;
  Process: THandle;
  Tries: Integer;
begin
  for Tries := 1 to 5 do
  begin
    Wnd := FindWindowByClassName('ShotTray');
    if Wnd = 0 then
      Exit;
    Pid := 0;
    GetWindowThreadProcessId(Wnd, Pid);
    Process := OpenProcess(PROCESS_TERMINATE or SYNCHRONIZE, False, Pid);
    if Process = 0 then
      Exit;
    TerminateProcess(Process, 0);
    WaitForSingleObject(Process, 5000);
    CloseHandle(Process);
  end;
end;

function InstalledExe: String;
begin
  Result := ExpandConstant('{app}\shot.exe');
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  EndShot;
  Result := '';
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  { Start at login stays as it was, now for the installed copy. }
  if (CurStep = ssPostInstall) and RegValueExists(HKCU, RunKey, RunValue) then
    RegWriteStringValue(HKCU, RunKey, RunValue, '"' + InstalledExe + '"');
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep <> usUninstall then
    Exit;
  EndShot;
  { Start at login goes only if it starts this copy. }
  if RegQueryStringValue(HKCU, RunKey, RunValue, Command)
    and SameText(RemoveQuotes(Command), InstalledExe) then
  begin
    RegDeleteValue(HKCU, RunKey, RunValue);
    RegDeleteValue(HKCU, ApprovedKey, RunValue);
  end;
end;
