; Setup wizard for devassist. Built with Inno Setup 6.3 or newer:
;
;   ISCC /DAppVersion=0.2.0 /DExePath=..\target\release\devassist.exe installer\devassist.iss
;
; installer\build.ps1 does this with the version from Cargo.toml.
;
; The wizard asks whether to install for this user only (no administrator
; rights needed) or for everyone. It adds a Start menu entry, optionally a
; desktop icon, and optionally puts the folder on PATH.
; The AppId must never change: it is how an update finds the old install.

#define AppName "DevAssist"
#define AppExe "devassist.exe"
#define Publisher "DevAssist"
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef ExePath
  #define ExePath "..\target\release\devassist.exe"
#endif

[Setup]
AppId={{1DBB2B4D-916B-47F1-821B-22FF796C63A2}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#Publisher}
DefaultDirName={autopf}\{#AppName}
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=devassist-{#AppVersion}-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ChangesEnvironment=yes
UninstallDisplayName={#AppName}
UninstallDisplayIcon={app}\{#AppExe}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "Add to PATH, so devassist works in any terminal"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"; Comment: "Manage local GitHub repositories"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Run]
Filename: "{app}\{#AppExe}"; Description: "Start {#AppName}"; Flags: nowait postinstall skipifsilent

[Code]
// PATH lives in the machine environment for an install for everyone, and
// in the user's own environment otherwise.

function PathRoot: Integer;
begin
  if IsAdminInstallMode then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function PathKey: String;
begin
#ifdef TestPathKey
  // Test builds edit a value under this key instead of the real PATH.
  Result := '{#TestPathKey}';
#else
  if IsAdminInstallMode then
    Result := 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment'
  else
    Result := 'Environment';
#endif
end;

// Position of Dir as a whole entry of the semicolon-separated Paths, or 0.
function PathPos(Paths, Dir: String): Integer;
begin
  Result := Pos(';' + Uppercase(Dir) + ';', ';' + Uppercase(Paths) + ';');
end;

procedure AddToPath(Dir: String);
var
  Paths: String;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Paths) then
  begin
    // A PATH that exists but cannot be read is left alone: writing now
    // would replace it with this one folder.
    if RegValueExists(PathRoot, PathKey, 'Path') then
      exit;
    Paths := '';
  end;
  if PathPos(Paths, Dir) > 0 then
    exit;
  if (Paths <> '') and (Paths[Length(Paths)] <> ';') then
    Paths := Paths + ';';
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Paths + Dir);
end;

procedure RemoveFromPath(Dir: String);
var
  Paths: String;
  P: Integer;
begin
  if not RegQueryStringValue(PathRoot, PathKey, 'Path', Paths) then
    exit;
  P := PathPos(Paths, Dir);
  if P = 0 then
    exit;
  // P counts in the string with a ';' put in front, so it is where the
  // entry starts in Paths itself. Take the entry and one separator out.
  Delete(Paths, P, Length(Dir) + 1);
  if (Paths <> '') and (Paths[Length(Paths)] = ';') then
    Delete(Paths, Length(Paths), 1);
  RegWriteExpandStringValue(PathRoot, PathKey, 'Path', Paths);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddToPath(ExpandConstant('{app}'));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RemoveFromPath(ExpandConstant('{app}'));
end;
