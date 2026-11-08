; The admin copy of the sandbox's set-up program (S15 tier B), which hooks.nsh includes at file
; scope. Elevated code runs the set-up program only from CLAUSEO_ADMIN_DIR
; (%ProgramFiles%\Clauseo\Sandbox), which only administrators can write, never from $INSTDIR, which
; a per-user install leaves writable by the lawyer's processes. The copy comes from this
; installer's own payload, before Clauseo or any agent has run.
;
; Per user (interactive, not /UPDATE), at the end of NSIS_HOOK_POSTINSTALL, the outer instance,
; running as the lawyer and unelevated, reads its own user SID from its token, creates the
; lawyer's data folder ($LOCALAPPDATA\${BUNDLEID}) if it is missing, says a UAC prompt is next
; (adminCopyAsk), and starts $EXEPATH itself again with "runas" and
; `/S /CLAUSEO_ADMIN_SECTION /CLAUSEO_OWNER_SID=<sid> /CLAUSEO_DATA_DIR=<folder> /D=$INSTDIR`.
; It first refuses while Clauseo runs (its main window, CLAUSEO_MAIN_WINDOW_CLASS, found after the
; template's own app-running check): adminCopyAppRunning, exit code 24, no prompt. It waits for
; the elevated instance up to 10 minutes. --adopt checks that its parent (the elevated instance)
; and that one's parent (this outer one) are alive, started in order and both $EXEPATH, and that
; the outer one is unelevated with --owner-sid as its token user. NSIS holds $EXEPATH open sharing
; only reads until it exits, which keeps other Windows accounts from rewriting, renaming or
; deleting it meanwhile; a process already running as the lawyer can close that handle in this
; process (DuplicateHandle) or inject into it and swap the file. That is decision 0049's residual
; ("What stays open"): any same-user code, a tier-C agent's included, since one can start code
; outside Clauseo's jobs. The running checks here are correctness guards, not security boundaries.
;
; The elevated instance runs CLAUSEO_ELEVATED_BRANCH (admin_elevated.nsh), which the repo's copy
; of Tauri's template (windows/nsis/installer.nsi, decision 0035) inserts first in .onInit, and
; quits there: before any plug-in (so nothing is loaded from a plug-in folder under the lawyer's
; %TEMP%), page or section of the template's. It refuses too while Clauseo's main window is there
; (exit 97, the same outcome).
;
; The outer instance then says set up (a details line), or shows one box: declined, blocked
; (this computer's settings refused the elevation), another account, or failed. The install has succeeded either way, with tier C. /S and /P can't ask: a
; details line, and exit code 22 for /S. /UPDATE (Tauri's updater) skips it without a word; S15
; PR 3's task refreshes the copy. Running the installer again by hand runs it again (--adopt is
; idempotent; `File` overwrites the copy, whose locked DACL gives Administrators and SYSTEM full
; control).
;
; The per-machine installer is elevated already: its NSIS_HOOK_POSTINSTALL writes the copy to the
; same folder, whatever $INSTDIR is, and runs `--adopt --per-machine` (CLAUSEO_ADMIN_COPY_MACHINE)
; every time, updates included, unless Clauseo's main window is there (exit 24, any mode). Its box
; shows only interactively, never under /S, /P or /UPDATE. It loads its plug-ins from its own
; folder under Program Files, made first in .onInit (CLAUSEO_MACHINE_START, safe_temp.nsh).

; The private arguments; the folder the copy goes to ($PROGRAMFILES64 is the 64-bit Program
; Files from this 32-bit installer, the folder --adopt reads from FOLDERID_ProgramFiles); and this
; file's folder, for `File`: sidecars.mjs writes the program to crates/desktop/binaries.
!define CLAUSEO_ADMIN_SWITCH "/CLAUSEO_ADMIN_SECTION"
!define CLAUSEO_OWNER_SID_OPTION "/CLAUSEO_OWNER_SID="
!define CLAUSEO_DATA_DIR_OPTION "/CLAUSEO_DATA_DIR="
!define CLAUSEO_ADMIN_DIR "$PROGRAMFILES64\Clauseo\Sandbox"
!define CLAUSEO_HOOKS_DIR "${__FILEDIR__}"
; Exit codes of `clauseo-sandbox-setup.exe --adopt` (SetupExit in clauseo-sandbox, S15 PR 2).
!define CLAUSEO_ADOPT_DONE 0
!define CLAUSEO_ADOPT_USAGE 1
!define CLAUSEO_ADOPT_OTHER_ACCOUNT 2
!define CLAUSEO_ADOPT_FAILED 3
!define CLAUSEO_ADOPT_LOCK_HELD 5
!define CLAUSEO_ADOPT_DATA_DIR 6
!define CLAUSEO_ADOPT_NOT_ELEVATED 7
!define CLAUSEO_ADOPT_NOT_ADMIN_COPY 8
!define CLAUSEO_ADOPT_DOWNGRADE 9
; The elevated instance exits with 100 + --adopt's code, so NSIS's own 1 (cancelled) and 2
; (aborted, as by .onInit's downgrade check) never read as --adopt's; with 99 when the program
; was not written or started, or the arguments it was given were refused.
!define CLAUSEO_ADMIN_EXIT_BASE 100
!define CLAUSEO_ADMIN_NOT_RUN 99
; 98: the record names a newer admin copy than this installer's, which stays (nothing written).
!define CLAUSEO_ADMIN_NEWER_KEPT 98
; 97: Clauseo is running (its main window is there); nothing written.
!define CLAUSEO_ADMIN_APP_RUNNING 97
; 96: the per-machine installer stopped waiting for --adopt (CLAUSEO_ADMIN_WAIT_MS).
!define CLAUSEO_ADMIN_TIMED_OUT 96
; 95: this run wrote the admin copy, --adopt couldn't be started, and the copy is gone: Windows
; security removed it as it was written. The installers say setupExeRemoved and set hooks.nsh's
; CLAUSEO_EXIT_SETUP_EXE_REMOVED (20), never under /P. A failed write is 99 instead, never this.
!define CLAUSEO_ADMIN_REMOVED 95
; Clauseo's main window class, set where lib.rs builds the window (MAIN_WINDOW_CLASS; a test holds
; them equal), so FindWindow finds Clauseo's main window and no other program's. Hidden and
; minimised windows count: FindWindow finds every top-level window on the desktop.
!define CLAUSEO_MAIN_WINDOW_CLASS "ClauseoMainWindow"
; How long the outer instance waits for the elevated one, and the per-machine installer for
; --adopt: 10 minutes, then a details line (adminCopyStillRunning); --adopt carries on by itself.
!define CLAUSEO_ADMIN_WAIT_MS 600000
; The machine's tier-B record (S15 PR 2, record_values.rs), REG_SZ values in the 64-bit view the
; template's SetContext sets: who placed the admin copy (a SID, or "machine" for the per-machine
; installer), the set-up's owner, and the version of the admin copy (its build's semver).
!define CLAUSEO_RECORD_KEY "SOFTWARE\Clauseo\Sandbox"
!define CLAUSEO_RECORD_COPY_OWNER "AdminCopyOwner"
!define CLAUSEO_RECORD_MACHINE "machine"
!define CLAUSEO_RECORD_OWNER_SID "OwnerSid"
!define CLAUSEO_RECORD_VERSION "AdminCopyVersion"
; Each user's profile folder, as Windows records it (HKLM, administrators only):
; <key>\<SID>\ProfileImagePath.
!define CLAUSEO_PROFILE_LIST "SOFTWARE\Microsoft\Windows NT\CurrentVersion\ProfileList"
; The installers' own exit codes (SetErrorLevel), clear of NSIS's 0 to 2 and of #638's 20 and 21
; (packaging-setup-exe-missing). Only the last code set is reported (docs/firm-it.md). Under /S:
; 22, a silent per-user install, which can't ask for the UAC prompt, skipped the admin copy; 23,
; the per-machine installer's --adopt failed, or was still running when the wait ended
; (CLAUSEO_ADMIN_TIMED_OUT), so the set-up didn't finish.
!define CLAUSEO_EXIT_ADMIN_COPY_SKIPPED 22
!define CLAUSEO_EXIT_ADMIN_COPY_FAILED 23
; 24: the install found Clauseo running and didn't set Safe workspace up. Per user only
; interactively (a /S per-user install exits 22 before it looks); per machine in every mode. 25,
; the per-machine installer couldn't make its temporary folder, is CLAUSEO_EXIT_TEMP_FAILED
; (safe_temp.nsh).
!define CLAUSEO_EXIT_APP_RUNNING 24
; 1 once a wait for the set-up timed out (outcome 7, or CLAUSEO_ADMIN_TIMED_OUT per machine): the
; finish page then unticks and hides its "Run Clauseo" box (CLAUSEO_FINISH_SHOW).
Var ClauseoAdminTimedOut

; The elevated installers' protected temporary folder, made first in each elevated path, and the
; elevated instance's branch, which the template inserts first in .onInit.
!include "${__FILEDIR__}\safe_temp.nsh"
!include "${__FILEDIR__}\uninstall_in_place.nsh"
!include "${__FILEDIR__}\admin_elevated.nsh"

; Writes the set-up program from this installer's payload into CLAUSEO_ADMIN_DIR; the error flag
; is set when the folder or the file could not be written. Only an elevated process can write
; there: the folder inherits Program Files' rules, and --adopt then locks it (protected, owner
; Administrators). The folder needs no link check: only administrators (and SYSTEM and
; TrustedInstaller) can create or change anything under Program Files, so no one else can have put
; a junction or link at Clauseo or Sandbox, and an administrator can already write anywhere.
; Never a copy of $INSTDIR's program nor anything through $TEMP, either of which could be swapped
; before it runs. SetOverwrite try: a file that can't be replaced (running, say) is skipped with
; the error flag set, never NSIS's Abort/Retry/Ignore box. ARCH is read here, at the insertion,
; since the template defines it after :36.
!macro CLAUSEO_ADMIN_EXTRACT
  !ifndef CLAUSEO_SETUP_TRIPLE
    !if "${ARCH}" == "x64"
      !define CLAUSEO_SETUP_TRIPLE "x86_64-pc-windows-msvc"
    !else if "${ARCH}" == "arm64"
      !define CLAUSEO_SETUP_TRIPLE "aarch64-pc-windows-msvc"
    !else
      !error "hooks.nsh: no sandbox set-up program is built for ${ARCH}"
    !endif
  !endif
  ClearErrors
  SetOutPath "${CLAUSEO_ADMIN_DIR}"
  ${IfNot} ${Errors}
    SetOverwrite try
    File "/oname=clauseo-sandbox-setup.exe" "${CLAUSEO_HOOKS_DIR}\..\binaries\clauseo-sandbox-setup-${CLAUSEO_SETUP_TRIPLE}.exe"
    SetOverwrite lastused
  ${EndIf}
!macroend

; The elevated branch's: CLAUSEO_ADMIN_EXTRACT, then `--adopt ARGS` from there with ExecWait,
; never through cmd.exe or anything else, as --adopt checks its parents. ExecWait has no time
; limit and the branch loads no plug-in, so the outer instance bounds its own wait on this one
; (CLAUSEO_ADMIN_WAIT_MS). $R8: the exit code, or CLAUSEO_ADMIN_NOT_RUN when nothing ran, or
; CLAUSEO_ADMIN_REMOVED when the copy was written but couldn't be started and is gone.
!macro CLAUSEO_ADMIN_COPY ARGS
  StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
  !insertmacro CLAUSEO_ADMIN_EXTRACT
  ${IfNot} ${Errors}
    ExecWait '"${CLAUSEO_ADMIN_DIR}\clauseo-sandbox-setup.exe" --adopt ${ARGS}' $R8
    ${If} ${Errors}
      StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
      ${IfNot} ${FileExists} "${CLAUSEO_ADMIN_DIR}\clauseo-sandbox-setup.exe"
        StrCpy $R8 ${CLAUSEO_ADMIN_REMOVED}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; The per-machine installer's: CLAUSEO_ADMIN_EXTRACT, then `--adopt ARGS` started directly
; (CreateProcessW, no window, in the admin folder, which SetOutPath made current) and waited for
; up to CLAUSEO_ADMIN_WAIT_MS. Plug-ins are the template's own there, loaded from the run's own
; folder under Program Files (safe_temp.nsh). $R8: the exit code,
; CLAUSEO_ADMIN_TIMED_OUT past the wait (it finishes by itself), CLAUSEO_ADMIN_REMOVED when the
; copy was written but couldn't be started and is gone, or CLAUSEO_ADMIN_NOT_RUN. A
; 32-bit STARTUPINFOW is 68 bytes (cb first), a PROCESS_INFORMATION 16 (hProcess, hThread, ...).
; Uses $1, $2, $R5, $R7 and $R9.
!macro CLAUSEO_ADMIN_COPY_BOUNDED ARGS
  StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
  !insertmacro CLAUSEO_ADMIN_EXTRACT
  ${IfNot} ${Errors}
    StrCpy $R9 '"${CLAUSEO_ADMIN_DIR}\clauseo-sandbox-setup.exe" --adopt ${ARGS}'
    System::Call '*(i 68, &i64) p .R5'
    System::Call '*(&i16) p .R7'
    System::Call 'kernel32::CreateProcessW(p 0, w R9, p 0, p 0, i 0, i 0x08000000, p 0, p 0, p R5, p R7) i .r1'
    ${If} $1 <> 0
      System::Call '*$R7(p .r1, p .r2)'
      System::Call 'kernel32::CloseHandle(p r2)'
      System::Call 'kernel32::WaitForSingleObject(p r1, i ${CLAUSEO_ADMIN_WAIT_MS}) i .r2'
      ${If} $2 = 258
        StrCpy $R8 ${CLAUSEO_ADMIN_TIMED_OUT}
      ${ElseIf} $2 = 0
        System::Call 'kernel32::GetExitCodeProcess(p r1, *i ${CLAUSEO_ADMIN_NOT_RUN} R8) i .r2'
        ${If} $2 = 0
          StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
        ${EndIf}
      ${EndIf}
      System::Call 'kernel32::CloseHandle(p r1)'
    ${ElseIfNot} ${FileExists} "${CLAUSEO_ADMIN_DIR}\clauseo-sandbox-setup.exe"
      StrCpy $R8 ${CLAUSEO_ADMIN_REMOVED}
    ${EndIf}
    System::Free $R5
    System::Free $R7
  ${EndIf}
!macroend

; Before any File into the admin folder, so a refused install changes nothing there, the rules
; --adopt enforces (exit 2 and 9), checked first. $R1 0 to go on; 1 another account: per user
; (OWNER given), AdminCopyOwner or OwnerSid is there and isn't OWNER exactly ("machine" included);
; per machine (OWNER ""), AdminCopyOwner is there and isn't "machine" (OwnerSid, a user's set-up,
; is normal there). 2: the admin copy is from a newer build than this installer
; (CLAUSEO_NEWER_THAN_THIS), so an older installer never puts an older copy back. 3: the record
; holds a value this check reads (AdminCopyOwner, OwnerSid or AdminCopyVersion, found by name
; among the key's values, compared as Windows compares names) that ReadRegStr can't read as text
; (another type, too long) or that is empty. --adopt fails on such a record too: an unreadable
; value is exit 3; an empty AdminCopyVersion is exit 9 (Downgrade), nothing written. A value that is
; missing checks nothing: the first install has no record, and a per-machine set-up or a removal
; leaves no OwnerSid; --adopt checks again. Plain NSIS, no plug-in or library function, as the
; elevated branch needs. Uses $R5, $R7, $R8 and $0 to $4.
!macro CLAUSEO_RECORD_CHECK OWNER
  StrCpy $R1 0
  ; The values by index until EnumRegValue fails (no key, or no more values). The key's default
  ; value enumerates with an empty name, so an empty name doesn't end the walk.
  StrCpy $0 0
  ${Do}
    ClearErrors
    EnumRegValue $1 HKLM "${CLAUSEO_RECORD_KEY}" $0
    ${If} ${Errors}
      ${ExitDo}
    ${EndIf}
    ${If} $1 == "${CLAUSEO_RECORD_COPY_OWNER}"
    ${OrIf} $1 == "${CLAUSEO_RECORD_OWNER_SID}"
    ${OrIf} $1 == "${CLAUSEO_RECORD_VERSION}"
      ClearErrors
      ReadRegStr $R5 HKLM "${CLAUSEO_RECORD_KEY}" "$1"
      ${If} ${Errors}
      ${OrIf} $R5 == ""
        StrCpy $R1 3
        ${ExitDo}
      ${EndIf}
    ${EndIf}
    IntOp $0 $0 + 1
  ${Loop}
  ${If} $R1 = 0
    ReadRegStr $R5 HKLM "${CLAUSEO_RECORD_KEY}" "${CLAUSEO_RECORD_COPY_OWNER}"
    ${If} $R5 != ""
      ${If} "${OWNER}" == ""
        ${If} $R5 S!= "${CLAUSEO_RECORD_MACHINE}"
          StrCpy $R1 1
        ${EndIf}
      ${ElseIf} $R5 S!= "${OWNER}"
        StrCpy $R1 1
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $R1 = 0
  ${AndIf} "${OWNER}" != ""
    ReadRegStr $R5 HKLM "${CLAUSEO_RECORD_KEY}" "${CLAUSEO_RECORD_OWNER_SID}"
    ${If} $R5 != ""
    ${AndIf} $R5 S!= "${OWNER}"
      StrCpy $R1 1
    ${EndIf}
  ${EndIf}
  ${If} $R1 = 0
    ReadRegStr $R5 HKLM "${CLAUSEO_RECORD_KEY}" "${CLAUSEO_RECORD_VERSION}"
    ${If} $R5 != ""
      !insertmacro CLAUSEO_NEWER_THAN_THIS
    ${EndIf}
  ${EndIf}
!macroend

; $R1 set to 2 when the version in $R5 (semver, as the record holds it) is newer than this
; installer's ${VERSION}: x, y and z compared in turn as numbers, then a release before a
; pre-release of the same x.y.z (two pre-releases of one x.y.z count as the same). Plain NSIS.
; Uses $R7, $R8 and $0 to $4.
!macro CLAUSEO_NEWER_THAN_THIS
  StrCpy $2 $R5
  StrCpy $3 "${VERSION}"
  StrCpy $4 0
  !insertmacro CLAUSEO_COMPARE_PART
  !insertmacro CLAUSEO_COMPARE_PART
  !insertmacro CLAUSEO_COMPARE_PART
  ; Same x.y.z: what is left starts with "-" for a pre-release.
  ${If} $4 = 0
    StrCpy $0 $2 1
    StrCpy $1 $3 1
    ${If} $1 == "-"
    ${AndIf} $0 != "-"
      StrCpy $4 1
    ${EndIf}
  ${EndIf}
  ${If} $4 = 1
    StrCpy $R1 2
  ${EndIf}
!macroend

; One part of x.y.z from each version ($2 the record's, $3 this installer's), into $R7 and $R8;
; $4 becomes 1 (the record's newer) or -1 at the first part that differs.
!macro CLAUSEO_COMPARE_PART
  !insertmacro CLAUSEO_TAKE_NUMBER $2 $R7
  !insertmacro CLAUSEO_TAKE_NUMBER $3 $R8
  ${If} $4 = 0
    ${If} $R7 > $R8
      StrCpy $4 1
    ${ElseIf} $R7 < $R8
      StrCpy $4 -1
    ${EndIf}
  ${EndIf}
!macroend

; NUM: the digits at the start of STR as a number (0 when there are none); STR loses them and one
; "." after them. A character is a digit when IntOp gives it back unchanged. Uses $0 and $1.
!macro CLAUSEO_TAKE_NUMBER STR NUM
  StrCpy ${NUM} 0
  ${Do}
    StrCpy $0 ${STR} 1
    IntOp $1 $0 + 0
    ${If} $0 == ""
    ${OrIf} $1 S!= $0
      ${ExitDo}
    ${EndIf}
    IntOp ${NUM} ${NUM} * 10
    IntOp ${NUM} ${NUM} + $0
    StrCpy ${STR} ${STR} "" 1
  ${Loop}
  StrCpy $0 ${STR} 1
  ${If} $0 == "."
    StrCpy ${STR} ${STR} "" 1
  ${EndIf}
!macroend

; $R2: this process's user SID as a string, or "" when it can't be read. The outer instance's,
; which runs as the lawyer. OpenProcessToken on GetCurrentProcess() (-1) with TOKEN_QUERY (8);
; TokenUser (1) is a TOKEN_USER, whose first member points at the SID (256 bytes hold the
; largest); ConvertSidToStringSidW's string is LocalFree'd. Uses $R4 to $R7.
!macro CLAUSEO_OWN_SID
  StrCpy $R2 ""
  System::Call 'advapi32::OpenProcessToken(p -1, i 8, *p .R4) i .R5'
  ${If} $R5 <> 0
    System::Call '*(&i256) p .R6'
    System::Call 'advapi32::GetTokenInformation(p R4, i 1, p R6, i 256, *i .R5) i .R7'
    ${If} $R7 <> 0
      System::Call '*$R6(p .R5)'
      System::Call 'advapi32::ConvertSidToStringSidW(p R5, *p .R7) i .R5'
      ${If} $R5 <> 0
        System::Call 'kernel32::lstrcpynW(w .R2, p R7, i ${NSIS_MAX_STRLEN})'
        System::Call 'kernel32::LocalFree(p R7)'
      ${EndIf}
    ${EndIf}
    System::Free $R6
    System::Call 'kernel32::CloseHandle(p R4)'
  ${EndIf}
!macroend

; The per-user installer asks for the admin section (header), at the end of a successful install.
; $R6 is the outcome: 0 set up (a details line), 1 failed, 2 another Windows account has it, 4
; the prompt declined, 9 the elevation refused by this computer's settings (each a box and a
; details line), 5 a newer copy kept (a details line), 6
; Clauseo running (a box, a details line and exit code 24, interactive only), 7 still running
; after 10 minutes (a details line; the finish page hides "Run Clauseo"), 8 the elevated instance
; wrote the copy and Windows security removed it (CLAUSEO_ADMIN_REMOVED: setupExeRemoved, a box,
; a details line and exit code 20).
!macro CLAUSEO_ASK_FOR_THE_ADMIN_SECTION
  Push $R2
  Push $R3
  Push $R4
  Push $R5
  Push $R6
  Push $R7
  Push $R8
  Push $R9
  ${If} $UpdateMode = 1
    ; Tauri's updater (/UPDATE, with /P or /S): never a prompt. S15 PR 3's task refreshes the copy.
  ${ElseIf} ${Silent}
  ${OrIf} $PassiveMode = 1
    DetailPrint "$(adminCopySkippedSilent)"
    ${If} ${Silent}
      SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_SKIPPED}
    ${EndIf}
  ${Else}
    StrCpy $R6 1
    StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
    ; The lawyer's identity, from this unelevated instance: its SID, and its data folder, made
    ; here as the lawyer when missing (the set-up needs it there, theirs).
    !insertmacro CLAUSEO_OWN_SID
    StrCpy $R3 "$LOCALAPPDATA\${BUNDLEID}"
    CreateDirectory "$R3"
    ; Clauseo still running after the template's own check (Section Install's
    ; CheckIfAppIsRunning): no prompt, outcome 6. FindWindow finds hidden windows too. A
    ; correctness guard, not a security boundary: it runs once, before a UAC prompt that can wait
    ; indefinitely.
    FindWindow $R5 "${CLAUSEO_MAIN_WINDOW_CLASS}"
    ${If} $R5 <> 0
      StrCpy $R6 6
      StrCpy $R2 ""
    ${EndIf}
    ${If} $R2 != ""
      MessageBox MB_OK|MB_ICONINFORMATION "$(adminCopyAsk)"
      ; ShellExecuteExW "runas" on this installer, as the uninstall hook starts --remove:
      ; SEE_MASK_NOCLOSEPROCESS (0x40) for the exit code, SEE_MASK_FLAG_NO_UI (0x400) for no
      ; error box of Windows' own (the consent prompt still shows), SEE_MASK_NOZONECHECKS
      ; (0x800000) so a downloaded installer the person already ran is not warned about again.
      ; /D= goes last (NSIS reads the rest of the line as the folder). A declined prompt fails the
      ; call with 1223 (?e), a person's No: outcome 4. Elevation this computer's settings refuse,
      ; 1260 (ERROR_ACCESS_DISABLED_BY_POLICY) or 5 (ERROR_ACCESS_DENIED, under some UAC
      ; settings), is outcome 9 (adminCopyBlocked). Any other failure is a failure. Every failure
      ; but 1223 also puts its code in a details line. The working folder is $SYSDIR, nothing the
      ; lawyer writes.
      !if ${NSIS_PTR_SIZE} > 4
        !error "hooks.nsh lays out SHELLEXECUTEINFOW for a 32-bit installer; lay it out again"
      !endif
      System::Call '*(&l4, i 0x800440, p $HWNDPARENT, w "runas", w "$EXEPATH", w "/S ${CLAUSEO_ADMIN_SWITCH} ${CLAUSEO_OWNER_SID_OPTION}$R2 ${CLAUSEO_DATA_DIR_OPTION}$R3 /D=$INSTDIR", w "$SYSDIR", i 0, p 0, p 0, p 0, p 0, i 0, p 0, p 0) p .R5'
      System::Call 'shell32::ShellExecuteExW(p R5) i .R7 ?e'
      Pop $R9
      ${If} $R7 <> 0
        DetailPrint "$(adminCopyWorking)"
        System::Call '*$R5(i, i, p, p, p, p, p, i, p, p, p, p, i, p, p .R7)'
        ${If} $R7 <> 0
          ; Up to 10 minutes; past that (WAIT_TIMEOUT, 258), outcome 7, and the elevated instance
          ; finishes by itself (--adopt checked this instance at its start). Only an ended wait
          ; (WAIT_OBJECT_0, 0) reads the code, seeded "not run".
          System::Call 'kernel32::WaitForSingleObject(p R7, i ${CLAUSEO_ADMIN_WAIT_MS}) i .R9'
          ${If} $R9 = 258
            StrCpy $R6 7
            StrCpy $ClauseoAdminTimedOut 1
          ${ElseIf} $R9 = 0
            System::Call 'kernel32::GetExitCodeProcess(p R7, *i ${CLAUSEO_ADMIN_NOT_RUN} R8) i .R9'
            ${If} $R9 = 0
              StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
            ${EndIf}
          ${EndIf}
          System::Call 'kernel32::CloseHandle(p R7)'
        ${EndIf}
      ${ElseIf} $R9 = 1223
        StrCpy $R6 4
      ${Else}
        ; Any other failure names its code in the details list, so a support screenshot shows it.
        ; 1260 is what the VM measured for a policy-refused elevation (a standard user with
        ; ConsentPromptBehaviorUser 0: ShellExecuteExW failed, last error 1260); 5 comes under
        ; other UAC settings. Both are outcome 9; anything else stays outcome 1.
        DetailPrint "Windows refused the elevation (error $R9)"
        ${If} $R9 = 1260
        ${OrIf} $R9 = 5
          StrCpy $R6 9
        ${EndIf}
      ${EndIf}
      System::Free $R5
    ${EndIf}
    ; A newer copy kept: this instance's own check (98), or --adopt's (100 + 9). Clauseo running:
    ; this instance's check (outcome 6 already) or the elevated one's (97). The copy written and
    ; then removed (95).
    ${If} $R8 = ${CLAUSEO_ADMIN_NEWER_KEPT}
      StrCpy $R6 5
    ${ElseIf} $R8 = ${CLAUSEO_ADMIN_APP_RUNNING}
      StrCpy $R6 6
    ${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}
      StrCpy $R6 8
    ${EndIf}
    ${If} $R6 <> 4
    ${AndIf} $R6 <> 5
    ${AndIf} $R6 <> 6
    ${AndIf} $R6 <> 7
    ${AndIf} $R6 <> 8
    ${AndIf} $R6 <> 9
      ; --adopt's own code from the instance's 100 + code; -1 when --adopt never ran (99, or
      ; NSIS's own 1 or 2). Every code is named: done, another account, and the rest failed.
      StrCpy $R9 -1
      ${If} $R8 >= ${CLAUSEO_ADMIN_EXIT_BASE}
      ${AndIf} $R8 < 200
        IntOp $R9 $R8 - ${CLAUSEO_ADMIN_EXIT_BASE}
      ${EndIf}
      ${Select} $R9
        ${Case} ${CLAUSEO_ADOPT_DONE}
          StrCpy $R6 0
        ${Case} ${CLAUSEO_ADOPT_OTHER_ACCOUNT}
          StrCpy $R6 2
        ${Case4} ${CLAUSEO_ADOPT_USAGE} ${CLAUSEO_ADOPT_FAILED} ${CLAUSEO_ADOPT_LOCK_HELD} ${CLAUSEO_ADOPT_DATA_DIR}
          StrCpy $R6 1
        ${Case2} ${CLAUSEO_ADOPT_NOT_ELEVATED} ${CLAUSEO_ADOPT_NOT_ADMIN_COPY}
          StrCpy $R6 1
        ${Case} ${CLAUSEO_ADOPT_DOWNGRADE}
          StrCpy $R6 5
        ${CaseElse}
          StrCpy $R6 1
      ${EndSelect}
    ${EndIf}
    ${If} $R6 = 0
      DetailPrint "$(adminCopyDone)"
    ${ElseIf} $R6 = 1
      DetailPrint "$(adminCopyFailed)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyFailed)"
    ${ElseIf} $R6 = 2
      DetailPrint "$(adminCopyOtherAccount)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyOtherAccount)"
    ${ElseIf} $R6 = 4
      DetailPrint "$(adminCopyDeclined)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyDeclined)"
    ${ElseIf} $R6 = 5
      DetailPrint "$(adminCopyNewerKept)"
    ${ElseIf} $R6 = 6
      DetailPrint "$(adminCopyAppRunning)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyAppRunning)"
      SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}
    ${ElseIf} $R6 = 7
      DetailPrint "$(adminCopyStillRunning)"
    ${ElseIf} $R6 = 8
      DetailPrint "$(setupExeRemoved)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(setupExeRemoved)"
      SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}
    ${ElseIf} $R6 = 9
      ; Only reached interactively: /S and /P skip the prompt above (adminCopySkippedSilent, 22).
      DetailPrint "$(adminCopyBlocked)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyBlocked)"
    ${EndIf}
  ${EndIf}
  Pop $R9
  Pop $R8
  Pop $R7
  Pop $R6
  Pop $R5
  Pop $R4
  Pop $R3
  Pop $R2
!macroend

; The per-machine installer's admin copy (header), every run. The copy goes to CLAUSEO_ADMIN_DIR
; whatever $INSTDIR is; --adopt takes any $INSTDIR that only SYSTEM, Administrators or
; TrustedInstaller can change. A box only interactively: never under /S (exit 23, also when the
; wait ends first), /P or /UPDATE. A newer copy already there stays, with a details line. Clauseo
; running touches nothing (exit 24 in every mode). Keeps $0 to $4 and $R1; uses $R5, $R7, $R8 and
; $R9, which NSIS_HOOK_POSTINSTALL keeps.
!macro CLAUSEO_ADMIN_COPY_MACHINE
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $R1
  ; Clauseo running in this session (its main window, hidden or minimised included): nothing
  ; written, exit 24, a box only interactively. In every mode, /S included: SYSTEM's run (Intune,
  ; session 0) sees no user's window, which is harmless; a lawyer's own /S run is refused. A
  ; correctness guard, not a security boundary, as in the per-user installer.
  FindWindow $R5 "${CLAUSEO_MAIN_WINDOW_CLASS}"
  ${If} $R5 <> 0
    DetailPrint "$(adminCopyAppRunning)"
    SetErrorLevel ${CLAUSEO_EXIT_APP_RUNNING}
    ${IfNot} ${Silent}
    ${AndIf} $PassiveMode <> 1
    ${AndIf} $UpdateMode <> 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyAppRunning)"
    ${EndIf}
  ${Else}
    !insertmacro CLAUSEO_RECORD_CHECK ""
    ${If} $R1 = 1
      ; Another user's per-user install placed the copy: --adopt would refuse (2); the copy stays.
      StrCpy $R8 ${CLAUSEO_ADOPT_OTHER_ACCOUNT}
      !insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
    ${ElseIf} $R1 = 2
      DetailPrint "$(adminCopyNewerKept)"
    ${ElseIf} $R1 = 3
      ; A record value it can't read: --adopt would fail (3); nothing written, a failed set-up.
      StrCpy $R8 ${CLAUSEO_ADOPT_FAILED}
      !insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
    ${Else}
      DetailPrint "$(adminCopyWorking)"
      !insertmacro CLAUSEO_ADMIN_COPY_BOUNDED '--per-machine --user-install-dir "$INSTDIR"'
      SetOutPath "$INSTDIR"
      !insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
    ${EndIf}
  ${EndIf}
  Pop $R1
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; The per-machine outcome of --adopt's code in $R8: done, a newer copy kept (9), still running past
; the wait (a details line, exit 23 under /S, and no "Run Clauseo"), the copy written and then
; removed by Windows security (CLAUSEO_ADMIN_REMOVED: a details line, exit 20 unless /P, a box only
; interactively), or failed (a write that failed included: 23 under /S, never 20).
!macro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
  ${If} $R8 = ${CLAUSEO_ADOPT_DONE}
    DetailPrint "$(adminCopyDone)"
  ${ElseIf} $R8 = ${CLAUSEO_ADOPT_DOWNGRADE}
    DetailPrint "$(adminCopyNewerKept)"
  ${ElseIf} $R8 = ${CLAUSEO_ADMIN_TIMED_OUT}
    DetailPrint "$(adminCopyStillRunning)"
    StrCpy $ClauseoAdminTimedOut 1
    ${If} ${Silent}
      SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}
    ${EndIf}
  ${ElseIf} $R8 = ${CLAUSEO_ADMIN_REMOVED}
    DetailPrint "$(setupExeRemoved)"
    ${If} $PassiveMode <> 1
      SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}
    ${EndIf}
    ${IfNot} ${Silent}
    ${AndIf} $PassiveMode <> 1
    ${AndIf} $UpdateMode <> 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(setupExeRemoved)"
    ${EndIf}
  ${Else}
    DetailPrint "$(adminCopyMachineFailed)"
    ${If} ${Silent}
      SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}
    ${ElseIf} $PassiveMode <> 1
    ${AndIf} $UpdateMode <> 1
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyMachineFailed)"
    ${EndIf}
  ${EndIf}
!macroend

; The finish page's show function (template blocks "finish-show" and "finish-show-function"): after
; a wait that timed out, the "Run Clauseo" box is unticked and hidden, so the installer doesn't
; start Clauseo while the set-up may still be running. $mui.FinishPage.Run is MUI2's box, declared
; by MUI_PAGE_FINISH, which comes before the function that inserts this.
!macro CLAUSEO_FINISH_SHOW
  ${If} $ClauseoAdminTimedOut = 1
    ${NSD_Uncheck} $mui.FinishPage.Run
    ShowWindow $mui.FinishPage.Run ${SW_HIDE}
  ${EndIf}
!macroend
