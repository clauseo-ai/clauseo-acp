; Clauseo's hooks into Tauri's NSIS installer (bundle.windows.nsis.installerHooks). Only the
; release overlay crates/desktop/release/windows.conf.json names this file, so it is in the
; Windows installers that release.yml builds and in nothing else. The template (tauri-cli 2.12.2,
; installer.nsi) inserts NSIS_HOOK_PREUNINSTALL first in `Section Uninstall` (lines 783-787),
; before its own CheckIfAppIsRunning (788) and its "delete app data" block (877-891).
;
; NSIS_HOOK_PREUNINSTALL runs only for a real uninstall: Tauri's installer runs the old
; uninstaller with /UPDATE during an update ($UpdateMode = 1), which keeps everything.
;
; 0. The app must be closed before anything else: the hook runs the template's own
;    CheckIfAppIsRunning (utils.nsh, which installer.nsi includes at line 26) first, so declining
;    its prompt aborts the uninstall before S15's removal or any deletion. The template's own call
;    at 788 then finds nothing running.
; 1. When the sandbox's set-up tool (S15, clauseo-sandbox-setup.exe) is beside the app, the
;    uninstaller asks it to remove what it set up, elevated, so a currentUser uninstall raises one
;    UAC prompt. It reads its records under the data folder's `sandbox` folder to take its access
;    rules back off the lawyer's folders. Until S15's binaries are bundled the file is never there
;    and this step does nothing.
; 2. When the "Also delete conversations, saved versions and settings" box is ticked, the app
;    deletes its data folders itself (`Clauseo.exe --delete-app-data`, delete_app_data.rs), never
;    following a link, and the box is cleared so the template's `RmDir /r` never runs: NSIS 3.11's
;    `RMDir /r` follows directory junctions, and one under the data folder emptied the lawyer's
;    folder it pointed at (0035, 2026-11-05). The app keeps the `sandbox` folder unless step 1
;    ran and exited 0 (--sandbox-removed), and keeps `sandbox\set-aside` whenever it holds the
;    lawyer's files. With the app missing nothing is deleted. Each outcome but a full deletion
;    says one line (English.nsh): the kept folder when one is there, never the whole folder with
;    the sandbox's records in it; else the whole folder.
;
; The words are in English.nsh (deleteAppDataFailed, deleteAppDataKept); the uninstall-declined
; line is S15a's.
;
; NSIS_HOOK_POSTINSTALL, the template's last step of `Section Install`: the per-machine installer
; creates $COMMONAPPDATA\Clauseo (C:\ProgramData\Clauseo, where a firm's policy.json goes; see
; docs/firm-it.md section 6) owned by Administrators, so a user who made the folder first cannot
; make a firm's policy be ignored. A folder that is already there is left as it is.
; The uninstaller leaves that folder, since a firm's policy may be in it.

; Exit codes of `Clauseo.exe --delete-app-data` (crates/desktop/src/delete_app_data.rs; a test
; holds them equal).
!define CLAUSEO_DELETED 0
!define CLAUSEO_KEPT_SANDBOX 2
!define CLAUSEO_KEPT_SET_ASIDE 3
!define CLAUSEO_KEPT_AND_FAILED 4

; Sets $R4 to 1 when PATH is a real folder: GetFileAttributesW says a folder (0x10) and no
; reparse point (0x400), so a junction or a link is never named as kept and the lawyer is never
; led through it to its target. INVALID_FILE_ATTRIBUTES (0xFFFFFFFF, -1 as the System plugin's
; i) is no folder. ${FileExists} would follow a link. Uses $R3; 1040 is 0x410, 16 is 0x10.
!macro CLAUSEO_REAL_FOLDER PATH
  StrCpy $R4 0
  System::Call 'kernel32::GetFileAttributesW(w "${PATH}") i .R3'
  ${If} $R3 <> -1
    IntOp $R3 $R3 & 1040
    ${If} $R3 = 16
      StrCpy $R4 1
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; 0. The app closed first.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

    ; 1. S15's removal, elevated. ExecShellWait gives no exit code, so ShellExecuteExW with
    ; "runas" and SEE_MASK_NOCLOSEPROCESS (0x40), then wait on the process and read its code.
    ; SEE_MASK_FLAG_NO_UI (0x400) keeps Windows' own error box away when the call fails; it does
    ; not suppress the consent prompt. A declined prompt (1223) or a policy that blocks the
    ; program (1260) fails the call, exactly like a failed --remove: $R5 stays 0 and the app
    ; keeps `sandbox\`. $R5 is 1 only when the removal ran and exited 0.
    StrCpy $R5 0
    ${If} ${FileExists} "$INSTDIR\clauseo-sandbox-setup.exe"
      ; SHELLEXECUTEINFOW as a 32-bit process lays it out: every member 4 bytes, no padding.
      !if ${NSIS_PTR_SIZE} > 4
        !error "hooks.nsh lays out SHELLEXECUTEINFOW for a 32-bit installer; lay it out again"
      !endif
      ; cbSize, fMask, hwnd, lpVerb, lpFile, lpParameters, lpDirectory, nShow (SW_HIDE),
      ; hInstApp, lpIDList, lpClass, hkeyClass, dwHotKey, hIcon, hProcess.
      System::Call '*(&l4, i 0x440, p $HWNDPARENT, w "runas", w "$INSTDIR\clauseo-sandbox-setup.exe", w "--remove", p 0, i 0, p 0, p 0, p 0, p 0, i 0, p 0, p 0) p .R6'
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
      ; The rest of the template's block for the box (installer.nsi 879-886), as it does it.
      DeleteRegKey SHCTX "${MANUPRODUCTKEY}"
      DeleteRegKey /ifempty SHCTX "${MANUKEY}"
      DeleteRegValue HKCU "${MANUPRODUCTKEY}" "Installer Language"
      DeleteRegKey /ifempty HKCU "${MANUPRODUCTKEY}"
      DeleteRegKey /ifempty HKCU "${MANUKEY}"
      ; The uninstalling user's local data folder, for the messages ($LOCALAPPDATA in the
      ; current user's context; a per-machine uninstall runs in the all-users one, put back
      ; after). The app finds the folders itself, from the known folders.
      StrCpy $R6 0
      ${If} ${ShellVarContextAll}
        StrCpy $R6 1
        SetShellVarContext current
      ${EndIf}
      StrCpy $R9 "$LOCALAPPDATA\${BUNDLEID}"
      ${If} $R6 = 1
        SetShellVarContext all
      ${EndIf}
      StrCpy $R7 1
      ${If} ${FileExists} "$INSTDIR\${MAINBINARYNAME}.exe"
        ClearErrors
        ${If} $R5 = 1
          ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-app-data --sandbox-removed' $R7
        ${Else}
          ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" --delete-app-data' $R7
        ${EndIf}
        ${If} ${Errors}
          StrCpy $R7 1
        ${EndIf}
      ${EndIf}
      ; The kept folder: the set-aside (3, or 4 after --sandbox-removed), else `sandbox` whenever
      ; it is still there (kept, or left by a failure, a refusal or a missing app), so the lawyer
      ; is never told to delete its records with the rest. Each only when it is a real folder
      ; (CLAUSEO_REAL_FOLDER; $R2 for the set-aside, $R4 for `sandbox`): a `sandbox` link the app
      ; kept (another kind of reparse point, or one it could not open) is never named, nor a
      ; linked set-aside. None: only the whole folder is named.
      StrCpy $R8 ""
      !insertmacro CLAUSEO_REAL_FOLDER "$R9\sandbox\set-aside"
      StrCpy $R2 $R4
      !insertmacro CLAUSEO_REAL_FOLDER "$R9\sandbox"
      ; The set-aside's attributes come through a `sandbox` link: it counts only under a real one.
      IntOp $R2 $R2 & $R4
      ${If} $R7 = ${CLAUSEO_KEPT_SET_ASIDE}
      ${AndIf} $R2 = 1
        StrCpy $R8 "$R9\sandbox\set-aside"
      ${ElseIf} $R7 = ${CLAUSEO_KEPT_AND_FAILED}
      ${AndIf} $R5 = 1
      ${AndIf} $R2 = 1
        StrCpy $R8 "$R9\sandbox\set-aside"
      ${ElseIf} $R4 = 1
        StrCpy $R8 "$R9\sandbox"
      ${EndIf}
      ; The data folder itself, by its own attributes (never through a link): $R6 0 when it is
      ; gone (INVALID_FILE_ATTRIBUTES with ERROR_FILE_NOT_FOUND 2 or ERROR_PATH_NOT_FOUND 3, ?e
      ; pushing GetLastError), 1 a folder that is no reparse point, 2 a reparse point or anything
      ; unreadable. The app's refusal of a data folder that is a link (#473) keeps that link and
      ; exits 1, so $R9 can be a surviving link here: that box names no path (a path, or a kept
      ; folder found through it, would lead through the link). Gone: no box, since nothing is left
      ; to look at (an unlink-only run, say: exit 1 with the whole folder deleted).
      System::Call 'kernel32::GetFileAttributesW(w "$R9") i .R3 ?e'
      Pop $R1
      StrCpy $R6 1
      ${If} $R3 = -1
        StrCpy $R6 2
        ${If} $R1 = 2
        ${OrIf} $R1 = 3
          StrCpy $R6 0
        ${EndIf}
      ${Else}
        IntOp $R3 $R3 & 1024
        ${If} $R3 <> 0
          StrCpy $R6 2
        ${EndIf}
      ${EndIf}
      ; One message: nothing kept, the whole folder (Failed); a folder kept and all else gone
      ; (Kept); a folder kept and something else left, both folders in one box (KeptFailed); the
      ; data folder a link or unreadable, no path (FailedUnnamed).
      ${IfNot} ${Silent}
      ${AndIf} $R7 <> ${CLAUSEO_DELETED}
      ${AndIf} $R6 <> 0
        ${If} $R6 = 2
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataFailedUnnamed)"
        ${ElseIf} $R8 == ""
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataFailed)"
        ${ElseIf} $R7 = ${CLAUSEO_KEPT_SANDBOX}
        ${OrIf} $R7 = ${CLAUSEO_KEPT_SET_ASIDE}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataKept)"
        ${Else}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(deleteAppDataKeptFailed)"
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; The firm-policy folder, per-machine installs only. The test is inside the macro: the template
; (installer.nsi) includes this file at line 36, before `!define INSTALLMODE` at line 46, so a
; test at file scope would see no INSTALLMODE.
!macro NSIS_HOOK_POSTINSTALL
  !if "${INSTALLMODE}" == "perMachine"
    Push $R5
    Push $R6
    Push $R7
    Push $R8
    Push $R9
    ; The all-users context, which the per-machine installer already runs in (SetContext);
    ; set for the work if not, and put back after ($R9 1).
    StrCpy $R9 0
    ${IfNot} ${ShellVarContextAll}
      StrCpy $R9 1
      SetShellVarContext all
    ${EndIf}
    ; Anything at the path, a link or a file included, is left as it is: GetFileAttributesW reads
    ; the path itself and never follows a link (${FileExists} would). INVALID_FILE_ATTRIBUTES is -1.
    System::Call 'kernel32::GetFileAttributesW(w "$COMMONAPPDATA\Clauseo") i .R7'
    ${If} $R7 <> -1
      DetailPrint "$(policyFolderExists)"
    ${Else}
      ; Created in one step with its security: owner Administrators (O:BA), group Administrators,
      ; a protected DACL (P: nothing inherited from ProgramData), SYSTEM and Administrators full
      ; access, Users read and execute (0x1200a9), each inherited by files and folders inside
      ; (OICI). SDDL_REVISION_1 is 1; the size out-parameter is optional (NULL).
      StrCpy $R6 0
      System::Call 'advapi32::ConvertStringSecurityDescriptorToSecurityDescriptorW(w "O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)", i 1, *p .R6, p 0) i .R7'
      StrCpy $R8 0
      ${If} $R7 <> 0
        ; SECURITY_ATTRIBUTES as a 32-bit process lays it out: every member 4 bytes, no padding.
        !if ${NSIS_PTR_SIZE} > 4
          !error "hooks.nsh lays out SECURITY_ATTRIBUTES for a 32-bit installer; lay it out again"
        !endif
        ; nLength, lpSecurityDescriptor, bInheritHandle (FALSE).
        System::Call '*(&l4, p R6, i 0) p .R7'
        System::Call 'kernel32::CreateDirectoryW(w "$COMMONAPPDATA\Clauseo", p R7) i .R8 ?e'
        ; GetLastError: ERROR_ALREADY_EXISTS (183) means something appeared there since the test.
        Pop $R5
        ${If} $R8 = 0
        ${AndIf} $R5 = 183
          StrCpy $R8 2
        ${EndIf}
        System::Free $R7
        ; The descriptor is the system's (LocalAlloc'd); LocalFree releases it.
        System::Call 'kernel32::LocalFree(p R6)'
      ${EndIf}
      ${If} $R8 = 2
        DetailPrint "$(policyFolderExists)"
      ${ElseIf} $R8 = 0
        DetailPrint "$(policyFolderFailed)"
      ${EndIf}
    ${EndIf}
    ${If} $R9 = 1
      SetShellVarContext current
    ${EndIf}
    Pop $R9
    Pop $R8
    Pop $R7
    Pop $R6
    Pop $R5
  !endif
!macroend
