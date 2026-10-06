﻿; mdview の Windows インストーラ（B-3 / B-4）。
;
; **利用者ごとに入れる。** %LOCALAPPDATA% へ置くので管理者権限が要らず、
; UAC も出ない。共用 PC で全利用者へ配る運用は採らない（§27.3）。
;
; **署名しない**（§27.4）。初回起動時に SmartScreen が出るため、
; 外し方を Release の説明文に入れる。
;
; 組み立て:
;   iscc /DMyAppVersion=2.0.0 /DMySourceExe=..\..\target\release\mdview.exe mdview.iss

#ifndef MyAppVersion
  #define MyAppVersion "0.0.0"
#endif
; **数字だけの版数。** `2.0.0-alpha.1` のような表記はリソースに入れられない
#ifndef MyFileVersion
  #define MyFileVersion "0.0.0.0"
#endif
#ifndef MySourceExe
  #define MySourceExe "..\..\target\release\mdview.exe"
#endif
#ifndef MyOutputDir
  #define MyOutputDir "..\..\dist"
#endif

#define MyAppName "mdview"
#define MyAppExe "mdview.exe"

[Setup]
; **AppId は変えない。** 変えると上書き更新ではなく二重に入る
AppId={{8E4C1F26-9A3B-4D57-9E2A-6B1F0C5D7A84}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
VersionInfoVersion={#MyFileVersion}
DefaultDirName={localappdata}\Programs\{#MyAppName}
DefaultGroupName={#MyAppName}
DisableProgramGroupPage=yes
; 置き場を選ばせる（既定のままでよい人はそのまま進める）
DisableDirPage=no
; **管理者権限を要らなくする**
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#MyOutputDir}
OutputBaseFilename=mdview-{#MyAppVersion}-windows-x86_64-setup
SetupIconFile=..\..\assets\icons\mdview.ico
UninstallDisplayIcon={app}\{#MyAppExe}
UninstallDisplayName={#MyAppName} {#MyAppVersion}
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
; 古い版が入っていれば先に消す
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
japanese.AssociateMd=.md / .markdown の「プログラムから開く」に {#MyAppName} を足す
english.AssociateMd=Add {#MyAppName} to the "Open with" list for .md / .markdown
japanese.LaunchAfter={#MyAppName} を起動する
english.LaunchAfter=Launch {#MyAppName}

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; Flags: unchecked
; **関連付けは選択制。** 既定では付けない（他の道具を使っている人の邪魔をしない）
Name: "associate"; Description: "{cm:AssociateMd}"; Flags: unchecked

[Files]
Source: "{#MySourceExe}"; DestDir: "{app}"; DestName: "{#MyAppExe}"; Flags: ignoreversion
Source: "..\..\assets\icons\mdview-doc.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExe}"; Tasks: desktopicon

[Registry]
; **HKCU へ書く。** 利用者ごとに入れるので、他の利用者へ影響しない
Root: HKCU; Subkey: "Software\Classes\{#MyAppName}.Document"; ValueType: string; ValueName: ""; ValueData: "Markdown 文書"; Flags: uninsdeletekey; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\{#MyAppName}.Document\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\mdview-doc.ico"; Flags: uninsdeletekey; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\{#MyAppName}.Document\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Flags: uninsdeletekey; Tasks: associate

; 拡張子ごと。**既定の値は書き換えない。**
;
; 書き換えても効かず、しかも壊す。Windows 8 以降は `UserChoice`（利用者が
; 自分で選んだ関連付け）が優先されるため書いても無視され、消すときに
; `uninsdeletevalue` で**元から入っていた値まで消してしまう**。
; 実機で確かめたところ、`.md` にはすでに別の道具の関連付けと `UserChoice`
; が入っていた。
;
; 代わりに `OpenWithProgids` へ**足す**。「プログラムから開く」の一覧に出て、
; 既定にするかどうかは利用者が「設定 > アプリ > 既定のアプリ」で決める。
Root: HKCU; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppName}.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "{#MyAppName}.Document"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

; 「プログラムから開く」の一覧へ出す
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#MyAppExe}"" ""%1"""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Applications\{#MyAppExe}\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""; Flags: uninsdeletekey

[Run]
Filename: "{app}\{#MyAppExe}"; Description: "{cm:LaunchAfter}"; Flags: nowait postinstall skipifsilent

[Code]
// **関連付けを変えたことをエクスプローラーへ知らせる。**
// 知らせないと、アイコンが次のログオンまで古いままになる
procedure SHChangeNotify(wEventId: Integer; uFlags: Cardinal;
  dwItem1, dwItem2: Cardinal);
  external 'SHChangeNotify@shell32.dll stdcall';

procedure RefreshShellIcons();
begin
  // SHCNE_ASSOCCHANGED / SHCNF_IDLIST
  SHChangeNotify($08000000, $0000, 0, 0);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    RefreshShellIcons();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    RefreshShellIcons();
end;
