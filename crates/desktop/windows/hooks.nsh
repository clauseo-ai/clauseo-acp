; Clauseo's hooks into Tauri's NSIS installer (bundle.windows.nsis.installerHooks). Only the
; release overlay crates/desktop/release/windows.conf.json names this file, so it is in the
; Windows installers that release.yml builds and in nothing else.
;
; NSIS_HOOK_PREUNINSTALL: when the sandbox's set-up tool (S15, clauseo-sandbox-setup.exe) is
; beside the app, the uninstaller asks it to remove what it set up, elevated, so a currentUser
; uninstall raises one UAC prompt. It runs only for a real uninstall: Tauri's installer runs the
; old uninstaller with /UPDATE during an update ($UpdateMode = 1), which keeps the set-up.
; Until S15's binaries are bundled the file is never there and the hook does nothing.
; This file holds no words: what the uninstaller says when the prompt is declined is S15a's.

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ${If} ${FileExists} "$INSTDIR\clauseo-sandbox-setup.exe"
      ExecShellWait "runas" "$INSTDIR\clauseo-sandbox-setup.exe" "--remove" SW_HIDE
    ${EndIf}
  ${EndIf}
!macroend
