; The admin copy of the sandbox's set-up program (S15 tier B), which hooks.nsh includes at file
; scope (so where the template includes hooks.nsh, installer.nsi:36). Elevated code runs the set-up
; program only from CLAUSEO_ADMIN_DIR (%ProgramFiles%\Clauseo\Sandbox), which only administrators
; can write, never from $INSTDIR, which a per-user install leaves writable by the lawyer's
; processes. The copy comes from this installer's own payload, before Clauseo or any agent has run.
;
; Per user (interactive, not /UPDATE), at the end of NSIS_HOOK_POSTINSTALL, the outer instance,
; running as the lawyer and unelevated, reads its own user SID from its token, creates the
; lawyer's data folder ($LOCALAPPDATA\${BUNDLEID}) if it is missing, says a UAC prompt is next
; (adminCopyAsk), and starts $EXEPATH itself again with "runas" and
; `/S /CLAUSEO_ADMIN_SECTION /CLAUSEO_OWNER_SID=<sid> /CLAUSEO_DATA_DIR=<folder> /D=$INSTDIR`.
; It waits for that instance however long it takes. --adopt checks that its parent (the elevated
; instance) and that one's parent (this outer one) are alive, started in order and both
; $EXEPATH, and that the outer one is unelevated with --owner-sid as its token user. NSIS holds
; $EXEPATH open sharing only reads until it exits, so nothing can rewrite, rename or delete it
; meanwhile.
;
; In the elevated instance, two things run, and nothing else of the template's after .onInit
; (which only reads switches and the registry, and has no single-instance or app-running check):
; - CLAUSEO_ADMIN_SECTION_ONLY, the script's first section, unselects every section but `Install`,
;   so Section WebView2 (installer.nsi:553-643) never runs. /UPDATE would skip only its install,
;   not its version check, whose Abort/Retry/Ignore box NSIS shows even under /S.
; - NSIS_HOOK_PREINSTALL, the first step of `Install` (installer.nsi:648), takes the owner's SID
;   and data folder from the arguments the outer instance passed, never from its own token or
;   environment (an administrator's, when someone else typed their password into the prompt),
;   writes the program from the payload and runs `--adopt` there (CLAUSEO_ADMIN_COPY), then quits
;   with 100 + its exit code, before the template's CheckIfAppIsRunning (:652) or any of its Files.
;   The section can't do this itself: ARCH, INSTALLMODE and BUNDLEID are defined after :36.
;
; The outer instance then says set up (a details line), or shows one box: declined, another
; account, or failed. The install has succeeded either way, with tier C. /S and /P can't ask: a
; details line, and exit code 22 for /S. /UPDATE (Tauri's updater) skips it without a word; S15
; PR 3's task refreshes the copy. Running the installer again by hand runs it again (--adopt is
; idempotent; `File` overwrites the copy, whose locked DACL gives Administrators and SYSTEM full
; control).
;
; The per-machine installer is elevated already: its NSIS_HOOK_POSTINSTALL writes the copy to the
; same folder, whatever $INSTDIR is, and runs `--adopt --per-machine` (CLAUSEO_ADMIN_COPY_MACHINE)
; every time, updates included. Its box shows only interactively, never under /S, /P or /UPDATE.

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
; The machine's tier-B record (S15 PR 2, record_values.rs), REG_SZ values in the 64-bit view the
; template's SetContext sets: who placed the admin copy (a SID, or "machine" for the per-machine
; installer), the set-up's owner, and the version of the admin copy (its build's semver).
!define CLAUSEO_RECORD_KEY "SOFTWARE\Clauseo\Sandbox"
!define CLAUSEO_RECORD_COPY_OWNER "AdminCopyOwner"
!define CLAUSEO_RECORD_MACHINE "machine"
!define CLAUSEO_RECORD_OWNER_SID "OwnerSid"
!define CLAUSEO_RECORD_VERSION "AdminCopyVersion"
; The installers' own exit codes under /S (SetErrorLevel), clear of NSIS's 0 to 2 and of #638's
; 20 and 21 (packaging-setup-exe-missing). 22: a silent per-user install, which can't ask for the
; UAC prompt, skipped the admin copy. 23: the per-machine installer's --adopt failed.
!define CLAUSEO_EXIT_ADMIN_COPY_SKIPPED 22
!define CLAUSEO_EXIT_ADMIN_COPY_FAILED 23

; $R0 1 when the switch is one of this instance's arguments, whole: GetOptions finds it after the
; program's path, and it carries no value (so not /CLAUSEO_ADMIN_SECTIONX nor ...=x). NSIS has
; already cut $CMDLINE before " /D=". Uses $R1.
!macro CLAUSEO_ADMIN_SWITCH_GIVEN
  StrCpy $R0 0
  ${GetParameters} $R1
  ClearErrors
  ${GetOptions} $R1 "${CLAUSEO_ADMIN_SWITCH}" $R1
  ${IfNot} ${Errors}
  ${AndIf} $R1 == ""
    StrCpy $R0 1
  ${EndIf}
!macroend

; The elevated instance runs no section of the template's but `Install` (header). The script's
; first section, since hooks.nsh comes before every section; hidden, so it always runs, and does
; nothing without the switch. An index past the last sets the error flag.
Section "-CLAUSEO_ADMIN_SECTION_ONLY"
  Push $R0
  Push $R1
  Push $R2
  !insertmacro CLAUSEO_ADMIN_SWITCH_GIVEN
  ${If} $R0 = 1
    StrCpy $R2 0
    ${Do}
      ClearErrors
      SectionGetText $R2 $R1
      ${If} ${Errors}
        ${ExitDo}
      ${EndIf}
      ${If} $R1 != "Install"
        SectionSetFlags $R2 0
      ${EndIf}
      IntOp $R2 $R2 + 1
    ${Loop}
  ${EndIf}
  Pop $R2
  Pop $R1
  Pop $R0
SectionEnd

; Writes the set-up program from this installer's payload into CLAUSEO_ADMIN_DIR and runs
; `--adopt ARGS` from there with ExecWait, never through cmd.exe or anything else, as --adopt
; checks its parents. $R8: its exit code, or CLAUSEO_ADMIN_NOT_RUN when the folder or the file
; could not be written or the program not started. Only an elevated process can write there: the
; folder inherits Program Files' rules, and --adopt then locks it (protected, owner
; Administrators). Never a copy of $INSTDIR's program nor anything through $TEMP, either of which
; could be swapped before it runs. SetOverwrite try: a file that can't be replaced (running, say)
; is skipped with the error flag set, never NSIS's Abort/Retry/Ignore box, and --adopt doesn't
; run. ARCH is read here, at the insertion, since the template defines it after :36.
!macro CLAUSEO_ADMIN_COPY ARGS
  !ifndef CLAUSEO_SETUP_TRIPLE
    !if "${ARCH}" == "x64"
      !define CLAUSEO_SETUP_TRIPLE "x86_64-pc-windows-msvc"
    !else if "${ARCH}" == "arm64"
      !define CLAUSEO_SETUP_TRIPLE "aarch64-pc-windows-msvc"
    !else
      !error "hooks.nsh: no sandbox set-up program is built for ${ARCH}"
    !endif
  !endif
  StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
  ClearErrors
  SetOutPath "${CLAUSEO_ADMIN_DIR}"
  ${IfNot} ${Errors}
    SetOverwrite try
    File "/oname=clauseo-sandbox-setup.exe" "${CLAUSEO_HOOKS_DIR}\..\binaries\clauseo-sandbox-setup-${CLAUSEO_SETUP_TRIPLE}.exe"
    SetOverwrite lastused
    ${IfNot} ${Errors}
      ExecWait '"${CLAUSEO_ADMIN_DIR}\clauseo-sandbox-setup.exe" --adopt ${ARGS}' $R8
      ${If} ${Errors}
        StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend

; Before any File into the admin folder, so a refused install changes nothing there, the rules
; --adopt enforces (exit 2 and 9), checked first. $R1 0 to go on; 1 another account: per user
; (OWNER given), AdminCopyOwner or OwnerSid is there and isn't OWNER exactly ("machine" included);
; per machine (OWNER ""), AdminCopyOwner is there and isn't "machine" (OwnerSid, a user's set-up,
; is normal there). 2: the admin copy is from a newer build than this installer
; (CLAUSEO_NEWER_THAN_THIS), so an older installer never puts an older copy back. A value that is
; missing or can't be read checks nothing; --adopt checks again. Uses $R5 to $R7 and $R9.
!macro CLAUSEO_RECORD_CHECK OWNER
  StrCpy $R1 0
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
  ${If} "${OWNER}" != ""
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
; installer's ${VERSION}: by its x.y.z (WordFunc's VersionCompare), then a release before a
; pre-release of the same x.y.z (two pre-releases of one x.y.z count as the same). In plain NSIS,
; no plug-in: the elevated instance loads as few DLLs from its plug-in folder as it can. Uses $R5
; to $R7 and $R9.
!macro CLAUSEO_NEWER_THAN_THIS
  StrCpy $R9 "${VERSION}"
  StrCpy $R7 0
  ${StrLoc} $R6 $R9 "-" ">"
  ${If} $R6 != ""
    StrCpy $R9 $R9 $R6
    StrCpy $R7 1
  ${EndIf}
  ${StrLoc} $R6 $R5 "-" ">"
  ${If} $R6 != ""
    StrCpy $R5 $R5 $R6
    IntOp $R7 $R7 - 1
  ${EndIf}
  ; $R7 1: this installer is a pre-release and the record's copy a release of the same x.y.z.
  ${VersionCompare} $R5 $R9 $R6
  ${If} $R6 = 1
    StrCpy $R1 2
  ${ElseIf} $R6 = 0
  ${AndIf} $R7 = 1
    StrCpy $R1 2
  ${EndIf}
!macroend

; $R0 set to 0 unless VALUE can stand in a quoted argument of --adopt's command line: not empty,
; no quote, not ending in a backslash (which would escape the closing quote). Uses $R5.
!macro CLAUSEO_CHECK_PATH VALUE
  StrCpy $R5 "${VALUE}" 1 -1
  ${If} "${VALUE}" == ""
  ${OrIf} $R5 == "\"
    StrCpy $R0 0
  ${EndIf}
  ${StrLoc} $R5 "${VALUE}" "$\"" ">"
  ${If} $R5 != ""
    StrCpy $R0 0
  ${EndIf}
!macroend

; $R0 set to 0 unless VALUE is a SID string: "S-1-", then only digits and dashes, at most 184
; characters (a SID's longest). Uses $R5, $R6 and $R7.
!macro CLAUSEO_CHECK_SID VALUE
  StrCpy $R5 "${VALUE}" 4
  StrLen $R6 "${VALUE}"
  ${IfNot} $R5 S== "S-1-"
  ${OrIf} $R6 > 184
    StrCpy $R0 0
  ${EndIf}
  StrCpy $R6 4
  ${Do}
    StrCpy $R5 "${VALUE}" 1 $R6
    ${If} $R5 == ""
      ${ExitDo}
    ${EndIf}
    ${StrLoc} $R7 "0123456789-" $R5 ">"
    ${If} $R7 == ""
      StrCpy $R0 0
    ${EndIf}
    IntOp $R6 $R6 + 1
  ${Loop}
!macroend

; The elevated instance of the per-user installer (header): with the switch it is the admin
; section only and quits here. Without it, the install goes on as the template has it. The
; per-machine installer never takes the switch.
!macro NSIS_HOOK_PREINSTALL
  !if "${INSTALLMODE}" == "currentUser"
    Push $R0
    Push $R1
    Push $R2
    Push $R3
    !insertmacro CLAUSEO_ADMIN_SWITCH_GIVEN
    ${If} $R0 = 1
      ; The owner, as the outer instance passed it, and $INSTDIR, its /D=. --adopt checks both
      ; against the outer instance's token and profile; these checks only keep the command line
      ; whole. A refused argument runs nothing (99).
      ${GetParameters} $R1
      ${GetOptions} $R1 "${CLAUSEO_OWNER_SID_OPTION}" $R2
      ${GetOptions} $R1 "${CLAUSEO_DATA_DIR_OPTION}" $R3
      StrCpy $R0 1
      !insertmacro CLAUSEO_CHECK_SID $R2
      !insertmacro CLAUSEO_CHECK_PATH $R3
      !insertmacro CLAUSEO_CHECK_PATH $INSTDIR
      StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
      ${If} $R0 = 1
        !insertmacro CLAUSEO_RECORD_CHECK $R2
        ${If} $R1 = 1
          IntOp $R8 ${CLAUSEO_ADMIN_EXIT_BASE} + ${CLAUSEO_ADOPT_OTHER_ACCOUNT}
        ${ElseIf} $R1 = 2
          StrCpy $R8 ${CLAUSEO_ADMIN_NEWER_KEPT}
        ${Else}
          !insertmacro CLAUSEO_ADMIN_COPY '--owner-sid $R2 --user-install-dir "$INSTDIR" --data-dir "$R3"'
          ${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}
            IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}
          ${EndIf}
        ${EndIf}
      ${EndIf}
      ; Quit runs no callback (.onInstSuccess, .onInstFailed): the exit code is this one.
      SetErrorLevel $R8
      Quit
    ${EndIf}
    Pop $R3
    Pop $R2
    Pop $R1
    Pop $R0
  !endif
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
; the prompt declined (each a box and a details line), 5 a newer copy kept (a details line).
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
    ${If} $R2 != ""
      MessageBox MB_OK|MB_ICONINFORMATION "$(adminCopyAsk)"
      ; ShellExecuteExW "runas" on this installer, as the uninstall hook starts --remove:
      ; SEE_MASK_NOCLOSEPROCESS (0x40) for the exit code, SEE_MASK_FLAG_NO_UI (0x400) for no
      ; error box of Windows' own (the consent prompt still shows), SEE_MASK_NOZONECHECKS
      ; (0x800000) so a downloaded installer the person already ran is not warned about again.
      ; /D= goes last (NSIS reads the rest of the line as the folder). A declined prompt fails the
      ; call with 1223 (?e); any other failure (1260, a blocked program) is a failure. The
      ; working folder is $SYSDIR, nothing the lawyer writes.
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
          ; However long it takes (INFINITE, -1): --adopt checks this instance is alive. Only
          ; an ended wait (WAIT_OBJECT_0, 0) reads the code, seeded "not run".
          System::Call 'kernel32::WaitForSingleObject(p R7, i -1) i .R9'
          ${If} $R9 = 0
            System::Call 'kernel32::GetExitCodeProcess(p R7, *i ${CLAUSEO_ADMIN_NOT_RUN} R8) i .R9'
            ${If} $R9 = 0
              StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
            ${EndIf}
          ${EndIf}
          System::Call 'kernel32::CloseHandle(p R7)'
        ${EndIf}
      ${ElseIf} $R9 = 1223
        StrCpy $R6 4
      ${EndIf}
      System::Free $R5
    ${EndIf}
    ; A newer copy kept: this instance's own check (98), or --adopt's (100 + 9).
    ${If} $R8 = ${CLAUSEO_ADMIN_NEWER_KEPT}
      StrCpy $R6 5
    ${EndIf}
    ${If} $R6 <> 4
    ${AndIf} $R6 <> 5
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
; TrustedInstaller can change. A box only interactively: never under /S (exit 23), /P or
; /UPDATE. A newer copy already there stays, with a details line. Uses $R1, $R5 and $R8.
!macro CLAUSEO_ADMIN_COPY_MACHINE
  !insertmacro CLAUSEO_RECORD_CHECK ""
  ${If} $R1 = 1
    ; Another user's per-user install placed the copy: --adopt would refuse (2), and the copy stays.
    StrCpy $R8 ${CLAUSEO_ADOPT_OTHER_ACCOUNT}
    !insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
  ${ElseIf} $R1 = 2
    DetailPrint "$(adminCopyNewerKept)"
  ${Else}
    DetailPrint "$(adminCopyWorking)"
    !insertmacro CLAUSEO_ADMIN_COPY '--per-machine --user-install-dir "$INSTDIR"'
    SetOutPath "$INSTDIR"
    !insertmacro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
  ${EndIf}
!macroend

; The per-machine outcome of --adopt's code in $R8: done, a newer copy kept (9), or failed.
!macro CLAUSEO_ADMIN_COPY_MACHINE_SAYS
  ${If} $R8 = ${CLAUSEO_ADOPT_DONE}
    DetailPrint "$(adminCopyDone)"
  ${ElseIf} $R8 = ${CLAUSEO_ADOPT_DOWNGRADE}
    DetailPrint "$(adminCopyNewerKept)"
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
