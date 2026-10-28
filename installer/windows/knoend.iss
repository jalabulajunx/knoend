; KnoEnd for Windows: a per-user installer (no administrator rights), built by Inno Setup in the
; release workflow:
;   iscc /DVersion=0.3.0 /DExe=path\to\knoend.exe installer\windows\knoend.iss
; It installs knoend.exe, adds "KnoEnd" to the Start menu (opens the web app; starts the service
; first when it is not running), and, if the person keeps the boxes ticked, runs KnoEnd in the
; background from login (`knoend service install`) and lets the browser extension reach it
; (`knoend browser install`). Uninstalling undoes both and stops KnoEnd.
;
; Not code-signed yet: Windows SmartScreen says "unknown publisher" (More info > Run anyway).

#ifndef Version
  #define Version "0.0.0"
#endif
#ifndef Exe
  #define Exe "..\..\companion\target\x86_64-pc-windows-msvc\release\knoend.exe"
#endif
#ifndef OutDir
  #define OutDir "..\..\out"
#endif

[Setup]
AppId={{54CA205C-1BC4-4F14-96B6-C495A57471BC}
AppName=KnoEnd
AppVersion={#Version}
AppVerName=KnoEnd {#Version}
AppPublisher=KnoEnd contributors
AppPublisherURL=https://github.com/jalabulajunx/knoend
AppSupportURL=https://github.com/jalabulajunx/knoend/issues
DefaultDirName={localappdata}\Programs\KnoEnd
DisableProgramGroupPage=yes
DisableDirPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutDir}
OutputBaseFilename=knoend-setup-{#Version}-x86_64
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE
UninstallDisplayIcon={app}\knoend.exe
UninstallDisplayName=KnoEnd
; A running knoend.exe holds its file: close it to update or remove it.
CloseApplications=force
RestartApplications=no

[Tasks]
Name: "background"; Description: "Run KnoEnd in the background, starting when I sign in"
Name: "browser"; Description: "Let the KnoEnd browser extension reach this computer (Chrome, Edge, Brave, Firefox)"

[Files]
Source: "{#Exe}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
; `knoend serve` opens the web app; when KnoEnd already runs, it opens that one's.
Name: "{userprograms}\KnoEnd"; Filename: "{app}\knoend.exe"; Parameters: "serve"; Comment: "Open the KnoEnd web app"

[Run]
Filename: "{app}\knoend.exe"; Parameters: "service install"; Tasks: background; Flags: runhidden waituntilterminated; StatusMsg: "Starting KnoEnd in the background..."
Filename: "{app}\knoend.exe"; Parameters: "browser install"; Tasks: browser; Flags: runhidden waituntilterminated; StatusMsg: "Connecting the browser extension..."
Filename: "{app}\knoend.exe"; Parameters: "serve"; Description: "Open KnoEnd now"; Flags: postinstall nowait skipifsilent runhidden

[UninstallRun]
Filename: "{app}\knoend.exe"; Parameters: "service uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "KnoEndService"
Filename: "{app}\knoend.exe"; Parameters: "browser uninstall"; Flags: runhidden waituntilterminated; RunOnceId: "KnoEndBrowser"
Filename: "{sys}\taskkill.exe"; Parameters: "/IM knoend.exe /F"; Flags: runhidden waituntilterminated; RunOnceId: "KnoEndStop"
