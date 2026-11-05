; Clauseo's hooks into Tauri's NSIS installer (bundle.windows.nsis.installerHooks). Only the
; release overlay crates/desktop/release/windows.conf.json names this file, so it is in the
; Windows installers that release.yml builds and in nothing else. The template (tauri-cli 2.12.2,
; installer.nsi) inserts NSIS_HOOK_PREUNINSTALL first in `Section Uninstall` (lines 783-787),
; before its own CheckIfAppIsRunning (788) and its "delete app data" block (877-891).
;
; NSIS_HOOK_PREUNINSTALL runs only for a real uninstall: Tauri's installer runs the old
; uninstaller with /UPDATE during an update ($UpdateMode = 1), which keeps everything.
;
; 1. When the sandbox's set-up tool (S15, clauseo-sandbox-setup.exe) is beside the app, the
;    uninstaller asks it to remove what it set up, elevated, so a currentUser uninstall raises one
;    UAC prompt. It runs first: it reads its records under the data folder's `sandbox` folder to
;    take its access rules back off the lawyer's folders. Until S15's binaries are bundled the
;    file is never there and this step does nothing.
; 2. When the "Also delete conversations, saved versions and settings" box is ticked, the app
;    deletes its data folders itself (`Clauseo.exe --delete-app-data`, delete_app_data.rs), never
;    following a link, and the box is cleared so the template's `RmDir /r` never runs: NSIS 3.11's
;    `RMDir /r` follows directory junctions, and one under the data folder emptied the lawyer's
;    folder it pointed at (0035, 2026-11-05). When step 1 failed (the prompt declined, or a
;    non-zero exit) the app keeps the `sandbox` folder (--keep-sandbox), so the removal can still
;    be done; it keeps `sandbox\set-aside` whenever it holds the lawyer's files. With the app
;    missing nothing is deleted. Each outcome but a full deletion says one line (English.nsh).
;
; The words are in English.nsh (deleteAppDataFailed, deleteAppDataKept); the uninstall-declined
; line is S15a's.

; Exit codes of `Clauseo.exe --delete-app-data` (crates/desktop/src/delete_app_data.rs; a test
; holds them equal).
!define CLAUSEO_DELETED 0
!define CLAUSEO_KEPT_SANDBOX 2
!define CLAUSEO_KEPT_SET_ASIDE 3

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; 1. S15's removal, elevated. ExecShellWait gives no exit code, so ShellExecuteExW with
    ; "runas" and SEE_MASK_NOCLOSEPROCESS (0x40), then wait on the process and read its code. A
    ; declined prompt fails the call. $R5 is 1 when the set-up is gone or was never here.
    StrCpy $R5 1
    ${If} ${FileExists} "$INSTDIR\clauseo-sandbox-setup.exe"
      StrCpy $R5 0
      ; SHELLEXECUTEINFOW as a 32-bit process lays it out: every member 4 bytes, no padding.
      !if ${NSIS_PTR_SIZE} > 4
        !error "hooks.nsh lays out SHELLEXECUTEINFOW for a 32-bit installer; lay it out again"
      !endif
      ; cbSize, fMask, hwnd, lpVerb, lpFile, lpParameters, lpDirectory, nShow (SW_HIDE),
      ; hInstApp, lpIDList, lpClass, hkeyClass, dwHotKey, hIcon, hProcess.
      System::Call '*(&l4, i 0x40, p $HWNDPARENT, w "runas", w "$INSTDIR\clauseo-sandbox-setup.exe", w "--remove", p 0, i 0, p 0, p 0, p 0, p 0, i 0, p 0, p 0) p .R6'
      System::Call 'shell32::ShellExecuteExW(p R6) i .R7'
      ${If} $R7 <> 0
        System::Call '*$R6(i, i, p, p, p, p, p, i, p, p, p, p, i, p, p .R7)'
        ${If} $R7 <> 0
          StrCpy $R8 1
          System::Call 'kernel32::WaitForSingleObject(p R7, i -1)'
          System::Call 'kernel32::GetExitCodeProcess(p R7, *i .R8)'
          System::Call 'kernel32::CloseHandle(p R7)'
          ${If} $R8 = 0
            StrCpy $R5 1
          ${EndIf}
        ${EndIf}
      ${EndIf}
      System::Free $R6
    ${EndIf}

    ; 2. The data folders, deleted by the app.
    ${If} $DeleteAppDataCheckboxState = 1
      ; First, whatever happens next: the template's `RmDir /r` never runs.
      StrCpy $DeleteAppDataCheckboxState 0
      ; The app's data goes only once the app is closed; the template checks only after this hook.
      !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
      ; The rest of the template's block for the box (installer.nsi 879-886), as it does it.
      DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
      DeleteRegKey /ifempty SHCTX "${MANUKEY}"
      DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
      DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
      DeleteRegKey /ifempty HKCU "${MANUKEY}"
      ; The folder the messages name. The app finds the folders itself, from the known folders.
      ReadEnvStr $R9 LOCALAPPDATA
      StrCpy $R9 "$R9\${BUNDLEID}"
      StrCpy $R7 1
      ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
        ClearErrors
        ${If} $R5 = 1
          ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-app-data' $R7
        ${Else}
          ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-app-data --keep-sandbox' $R7
        ${EndIf}
        ${If} ${Errors}
          StrCpy $R7 1
        ${EndIf}
      ${EndIf}
      ${If} $R7 = ${CLAUSEO_KEPT_SANDBOX}
        StrCpy $R8 "$R9\sandbox"
      ${Else}
        StrCpy $R8 "$R9\sandbox\set-aside"
      ${EndIf}
      ${IfNot} ${Silent}
        ${If} $R7 = ${CLAUSEO_KEPT_SANDBOX}
        ${OrIf} $R7 = ${CLAUSEO_KEPT_SET_ASIDE}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataKept)"
        ${ElseIf} $R7 <> ${CLAUSEO_DELETED}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataFailed)"
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
