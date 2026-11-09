; Clauseo's hooks into Tauri's NSIS installer (bundle.windows.nsis.installerHooks). Only the
; release overlay crates/desktop/release/windows.conf.json names this file, so it is in the
; Windows installers that release.yml builds and in nothing else. The template (tauri-cli 2.12.2,
; installer.nsi) inserts NSIS_HOOK_PREUNINSTALL first in `Section Uninstall` (lines 783-787),
; before its own CheckIfAppIsRunning (789) and its "delete app data" block (877-891). Line
; numbers are upstream's; the repo's copy (windows/nsis/installer.nsi, bundle.windows.nsis.template)
; adds one marked block at the top of .onInit (decision 0035).
;
; NSIS_HOOK_PREUNINSTALL runs only for a real uninstall: Tauri's installer runs the old
; uninstaller with /UPDATE during an update ($UpdateMode = 1), which keeps everything.
;
; 0. The app must be closed before anything else: the hook runs the template's own
;    CheckIfAppIsRunning (utils.nsh, which installer.nsi includes at line 26) first, so declining
;    its prompt aborts the uninstall before S15's removal or any deletion. The template's own call
;    at 789 then finds nothing running.
; 1. When the sandbox's set-up tool (S15, clauseo-sandbox-setup.exe) is installed, the
;    uninstaller asks it to remove what it set up, elevated, so a currentUser uninstall raises one
;    UAC prompt. The installer places an admin-only copy in `%ProgramFiles%\Clauseo\Sandbox\`, and
;    the hook starts that copy only: a per-user copy beside the app is never started elevated. The
;    tool reads its records under the data folder's `sandbox` folder to take its access rules back
;    off the lawyer's folders. sidecars.mjs bundles it in the Windows installers.
;    Before starting it the hook checks the copy as the app does (admin_copy.rs): every folder from
;    the drive's root down to it, and the copy, admin-only; then the copy's SHA-256, computed in
;    this process (bcrypt), the one the machine's record holds (AdminCopySha256). Anything else, or
;    anything it can't read or compute, and it refuses: nothing is started, `sandbox\` is kept,
;    and under /S exit code CLAUSEO_EXIT_ADMIN_COPY_REFUSED. It says the hidden accounts are still
;    there (removeAdminCopyRefused: a details line, and a box unless /S) only when the record names
;    an owner, per user the uninstalling user (CLAUSEO_OWNER_TO_TELL); else a details line only.
;    The start looks its verb up in HKLM\SOFTWARE\Classes\exefile alone (SEE_MASK_CLASSKEY),
;    never in HKCU\Software\Classes, which the uninstalling user can write.
;    Both pass `--uninstall`, which the app's in-app removal never does: it also takes the admin
;    copy, its record values and the tasks \Clauseo\SandboxRefresh, \Clauseo\SandboxStop and
;    \Clauseo\SandboxSweep away, last, for the install that placed them.
;    The per-user installer passes `--remove --uninstall`: only the Windows user who set the
;    protection up removes it; anyone else's uninstall exits 0 having touched nothing. The
;    per-machine installer passes `--remove --uninstall --per-machine` (chosen when it is built,
;    from the template's INSTALLMODE): every sandbox user goes, whoever uninstalls, LOCAL SYSTEM
;    under Intune's silent /S included.
;    A silent per-user uninstall (/S) never raises UAC: it skips the removal, as after a declined
;    prompt, and says so in its details list.
;    The wait is bounded at 2 minutes; past that the uninstall carries on as after a failed
;    removal and never stops the tool, which finishes later and leaves its records consistent at
;    every step. Only an ended wait (WAIT_OBJECT_0), an exit code read and that code 0 count as
;    removed, or 10 (CLAUSEO_REMOVED_COPY_LEFT): removed, with the admin copy left. A removal
;    that didn't finish (any exit but 0 or 10) keeps `sandbox\` and the machine's record; the
;    recovery is to reinstall Clauseo, then uninstall it again (decision 0049).
;    With the tool missing (Windows security can quarantine it: Defender called it
;    Trojan:Win32/Bearfoos.A!ml, a false positive) nothing can be removed. When the machine's
;    record (HKLM\SOFTWARE\Clauseo\Sandbox, 64-bit view) shows a set-up, the uninstaller says so
;    (removeExeMissing): a box, or under /S a line in the details list and exit code
;    CLAUSEO_EXIT_REMOVE_EXE_MISSING, so the protection's accounts are never left unsaid.
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
; The words are in English.nsh and Hindi.nsh (deleteAppDataFailed, deleteAppDataKept,
; removeStillRunning, removeSkippedSilent, removeExeMissing, removeAdminCopyRefused,
; setupExeRemoved, and admin_copy.nsh's adminCopy* lines); the uninstall-declined line is S15a's.
;
; NSIS_HOOK_POSTINSTALL, the template's last step of `Section Install`: the per-machine installer
; creates $APPDATA\Clauseo (C:\ProgramData\Clauseo, where a firm's policy.json goes; see
; docs/firm-it.md section 6) owned by Administrators, so a user who made the folder first cannot
; make a firm's policy be ignored. $APPDATA is the all-users folder only because the hook sets the
; context to all around its work (NSIS has no COMMONAPPDATA; the old name was taken as text).
; A folder that is already there is left as it is.
; The uninstaller leaves that folder, since a firm's policy may be in it.
; On an ARM64 computer the x64 per-machine installer also names the arm64 one in a details line
; (CLAUSEO_NATIVE_ARM64_NOTICE).
; Windows security can remove clauseo-sandbox-setup.exe as it is written. Launches run only the
; admin copy, so only a run that wrote that copy says it was removed (setupExeRemoved, exit code
; CLAUSEO_EXIT_SETUP_EXE_REMOVED): when --adopt couldn't be started and the file is gone
; (CLAUSEO_ADMIN_REMOVED, admin_copy.nsh). The install stays. The copy in $INSTDIR isn't checked:
; without it only the next refresh of the admin copy (S15 PR 3) is held up, which the app reports.
;
; The language (English or Hindi, windows/nsis/*.nsh). NSIS fixes the pages' language after
; .onInit and un.onInit (NSIS 3.11 Source/exehead/Ui.c:396-397, set_language; Docs/src/var.but:54
; "You can only change this variable in .onInit"), and the template gives neither a hook, so
; MUI_CUSTOMFUNCTION_GUIINIT (Ui.c:525, after set_language) would be too late. So the template's
; block "ui-language" in .onInit inserts CLAUSEO_UI_LANGUAGE (below): the installer speaks the
; language the lawyer chose in the app when there is one, else Windows' display language (NSIS's
; own choice), English otherwise. The uninstaller's un.onInit runs MUI_UNGETLANGUAGE
; (installer.nsi:770), which reads HKCU\${MANUPRODUCTKEY} "Installer Language"
; (installer.nsi:164-166) and, when that is empty and
; the uninstall is not silent, shows LangDLL's language picker (two languages). So
; NSIS_HOOK_POSTINSTALL writes that value at every install and update: the language the lawyer
; chose in the app (HKCU\Software\Clauseo\UiLanguage, `en` or `hi`, which the app writes), else
; the installer's own. The template deletes it with the data (installer.nsi:884, and step 2).
;
; Repair (/CLAUSEO_REPAIR, the per-user installer only; the per-machine one ignores it): the
; desktop app's "Download and run the installer again" button (repair_install, #777) runs this
; version's per-user installer interactively with exactly that switch, then closes its windows and
; exits within about 6 s. NSIS_HOOK_PREINSTALL, which the template inserts right before its own
; CheckIfAppIsRunning (installer.nsi:691-695), waits first up to 10 s, every 250 ms, until both
; Clauseo.exe in $INSTDIR (the template's own test: Restart Manager's list for the file) and any
; Clauseo main window (CLAUSEO_MAIN_WINDOW_CLASS) are gone; still there at 10 s, it carries on and
; the usual checks apply (the template's box, admin_copy.nsh's adminCopyAppRunning and exit 24,
; which comes later still, in NSIS_HOOK_POSTINSTALL). NSIS_HOOK_POSTINSTALL ends, after the admin
; step whatever its outcome, by starting Clauseo once as the user, as the finish page's "Run
; Clauseo" does (RunAsUser), and by taking the template's passive path from there: $PassiveMode 1,
; so the template's SkipIfPassive skips the finish page (no second start from its box) and
; SetAutoClose closes the installer. Only an install that ran to the end of Section Install gets
; there: a cancelled "Clauseo is running" box, a failed file, any Abort or a closed installer
; never reaches it, so nothing starts. A wait for the elevated instance that timed out
; ($ClauseoAdminTimedOut) keeps the usual finish page, its "Run Clauseo" hidden, and starts
; nothing, as without the switch. .onInstSuccess runs RunAsUser too under $PassiveMode, but only
; with /R, which the app never passes.

; NSIS's warning 6000 is "unknown variable/constant detected, ignoring" (Source/build.h,
; DW_VAR_IGNORED_UNKNOWN): an unknown name stays as literal text and the hook does the wrong thing
; quietly. This makes that warning an error for the whole compile (the template includes this file
; before its own code), so it comes before admin_copy.nsh and the files that one includes. The
; source test the_installer_scripts_use_only_known_nsis_variables checks the names as well.
!pragma warning error 6000 ; an unknown variable name is an error, not a silent literal (the COMMONAPPDATA bug)

; The admin copy of the sandbox's set-up program (S15 tier B) is in admin_copy.nsh, included
; here: the elevated instance's branch, which the template's .onInit block inserts
; (admin_elevated.nsh), and the macros NSIS_HOOK_POSTINSTALL ends with.
!include "${__FILEDIR__}\admin_copy.nsh"

; Repair mode (the paragraph "Repair" above): the switch, how long the installer waits for
; Clauseo to go and how often it looks, and 1 when the per-user installer was given the switch.
!define CLAUSEO_REPAIR_SWITCH "/CLAUSEO_REPAIR"
!define CLAUSEO_REPAIR_WAIT_MS 10000
!define CLAUSEO_REPAIR_POLL_MS 250
Var ClauseoRepair

; Exit codes of `Clauseo.exe --delete-app-data` (crates/desktop/src/delete_app_data.rs; a test
; holds them equal).
!define CLAUSEO_DELETED 0
!define CLAUSEO_KEPT_SANDBOX 2
!define CLAUSEO_KEPT_SET_ASIDE 3
!define CLAUSEO_KEPT_AND_FAILED 4

; The installers' own exit codes (SetErrorLevel), listed in docs/firm-it.md, "If Windows security
; removed Clauseo's set-up program": 20 set by admin_copy.nsh's macros (never under /P), 21 by the
; uninstaller under /S. Tauri's template sets none, and NSIS uses 0, 1
; (cancelled) and 2 (aborted); these stay clear of those and of the codes Intune and SCCM read as
; success or retry (1223, 1618, 1641, 1707, 3010).
!define CLAUSEO_EXIT_SETUP_EXE_REMOVED 20
!define CLAUSEO_EXIT_REMOVE_EXE_MISSING 21
; 22 to 26 are the installers' and the per-machine uninstaller's (admin_copy.nsh, safe_temp.nsh,
; uninstall_in_place.nsh).
!define CLAUSEO_EXIT_ADMIN_COPY_REFUSED 27

; The setup exe's --remove --uninstall exit 10 (SetupExit::AdminCopyLeft): the protection is
; removed, but the admin copy, or part of it, stays; the tool wrote the step and its error code
; to the Event Log. What is left is only administrators' to change, so the hook counts it as
; removed, as it does 0.
!define CLAUSEO_REMOVED_COPY_LEFT 10

; The rights admin_copy.rs's chain rule counts: CHANGE_RIGHTS on every level below the drive's
; root, ROOT_CHANGE_RIGHTS on the root (a test holds them equal); and TrustedInstaller's SID,
; which may own or change a level, as SYSTEM (S-1-5-18) and Administrators (S-1-5-32-544) may.
!define CLAUSEO_CHANGE_RIGHTS 0x520D0146
!define CLAUSEO_ROOT_CHANGE_RIGHTS 0x120C0040
!define CLAUSEO_TRUSTED_INSTALLER "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"

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

; Sets $1 to 1 when the SID at address SID is SYSTEM's, Administrators' or TrustedInstaller's,
; read as text (ConvertSidToStringSidW, its string copied out, then freed). Uses $R7 and $R8.
!macro CLAUSEO_TRUSTED_SID SID
  StrCpy $1 0
  StrCpy $R7 0
  System::Call 'advapi32::ConvertSidToStringSidW(p ${SID}, *p .R7) i .R8'
  ${If} $R8 <> 0
  ${AndIf} $R7 <> 0
    System::Call 'kernel32::lstrcpynW(w .R8, p R7, i ${NSIS_MAX_STRLEN})'
    System::Call 'kernel32::LocalFree(p R7)'
    ${If} $R8 == "S-1-5-18"
    ${OrIf} $R8 == "S-1-5-32-544"
    ${OrIf} $R8 == "${CLAUSEO_TRUSTED_INSTALLER}"
      StrCpy $1 1
    ${EndIf}
  ${EndIf}
!macroend

; One level of the admin copy's chain, as admin_copy.rs's level_safe and root_safe read it: the
; path in $4 is a folder ($7 16) or a file ($7 0) and no reparse point, by its own attributes;
; GetNamedSecurityInfoW (SE_FILE_OBJECT 1, owner and DACL 5) gives a trusted owner and a DACL
; (a NULL one allows everyone everything); and no allow entry that applies to the level itself
; (INHERIT_ONLY 8 skipped) gives a right in $6 to an untrusted SID. An allow entry whose SID
; follows its mask (types 0 and 9) is read; one whose SID isn't (4, 5 and 11) counts as an
; untrusted SID's. Deny entries are skipped. Clears $5 when the level isn't safe. Uses $1, $8, $9,
; $R0 to $R2 and $R6 to $R9.
!macro CLAUSEO_LEVEL_SAFE
  System::Call 'kernel32::GetFileAttributesW(w r4) i .r1'
  ${If} $1 = -1
    StrCpy $5 0
  ${Else}
    IntOp $1 $1 & 1040
    ${If} $1 <> $7
      StrCpy $5 0
    ${EndIf}
  ${EndIf}
  ${If} $5 = 1
    StrCpy $R0 0
    StrCpy $9 0
    System::Call 'advapi32::GetNamedSecurityInfoW(w r4, i 1, i 5, *p .r8, p 0, *p .r9, p 0, *p .R0) i .r1'
    ${If} $1 <> 0
    ${OrIf} $9 = 0
      StrCpy $5 0
    ${Else}
      !insertmacro CLAUSEO_TRUSTED_SID $8
      ${If} $1 <> 1
        StrCpy $5 0
      ${EndIf}
    ${EndIf}
    ${If} $5 = 1
      ; ACL: AclRevision, Sbz1, AclSize, then AceCount. Each ACE: AceType, AceFlags, AceSize,
      ; then the mask, then (types 0 and 9) the SID.
      System::Call '*$9(&i1, &i1, &i2, &i2 .R1)'
      IntOp $R1 $R1 - 1
      ${For} $R2 0 $R1
        System::Call 'advapi32::GetAce(p r9, i R2, *p .R6) i .r1'
        ${If} $1 = 0
          StrCpy $5 0
          ${ExitFor}
        ${EndIf}
        System::Call '*$R6(&i1 .R7, &i1 .R8, &i2, i .R9)'
        IntOp $R8 $R8 & 8
        IntOp $R9 $R9 & $6
        ${If} $R8 = 0
        ${AndIf} $R9 <> 0
          StrCpy $1 0
          ${If} $R7 = 0
          ${OrIf} $R7 = 9
            StrCpy $1 1
          ${EndIf}
          ${If} $R7 = 4
          ${OrIf} $R7 = 5
          ${OrIf} $R7 = 11
            StrCpy $5 0
          ${EndIf}
          ${If} $1 = 1
            IntOp $R6 $R6 + 8
            !insertmacro CLAUSEO_TRUSTED_SID $R6
            ${If} $1 <> 1
              StrCpy $5 0
            ${EndIf}
          ${EndIf}
          ${If} $5 = 0
            ${ExitFor}
          ${EndIf}
        ${EndIf}
      ${Next}
    ${EndIf}
    ${If} $R0 <> 0
      System::Call 'kernel32::LocalFree(p R0)'
    ${EndIf}
  ${EndIf}
!macroend

; The SHA-256 of the copy at $0, computed in this process (bcrypt.dll from $SYSDIR, by its full
; path, through System::Call), never by a program started for it: AppLocker, WDAC or an ASR rule
; may block one. The file opened for reading (FILE_GENERIC_READ 0x120089), shared for reading only
; (FILE_SHARE_READ 1), OPEN_EXISTING (3), so no one writes it while it is read; read in 64 KiB
; chunks (ReadFile) into BCryptHashData; then BCryptFinishHash's 32 bytes as 64 lowercase hex
; digits in $4. $2 is 1 when every call succeeded (an NTSTATUS of 0, a ReadFile that didn't fail),
; else 0 and $4 "". Each handle and the buffer are given back on every path: the file
; (CloseHandle), the hash (BCryptDestroyHash), the algorithm (BCryptCloseAlgorithmProvider), the
; buffer (System::Free). Uses $1 and $5 to $7, and $R6 to $R9.
!macro CLAUSEO_SHA256_OF_COPY
  StrCpy $2 0
  StrCpy $4 ""
  StrCpy $R6 0
  System::Call '$SYSDIR\bcrypt.dll::BCryptOpenAlgorithmProvider(*p .R6, w "SHA256", p 0, i 0) i .r1'
  ${If} $1 = 0
    StrCpy $R7 0
    System::Call '$SYSDIR\bcrypt.dll::BCryptCreateHash(p R6, *p .R7, p 0, i 0, p 0, i 0, i 0) i .r1'
    ${If} $1 = 0
      System::Call 'kernel32::CreateFileW(w r0, i 0x120089, i 1, p 0, i 3, i 0, p 0) p .R8'
      ${If} $R8 <> -1
        System::Alloc 65536
        Pop $R9
        ${If} $R9 <> 0
          StrCpy $5 1
          ${Do}
            System::Call 'kernel32::ReadFile(p R8, p R9, i 65536, *i .r6, p 0) i .r1'
            ${If} $1 = 0
              StrCpy $5 0
              ${ExitDo}
            ${EndIf}
            ${If} $6 = 0
              ${ExitDo}
            ${EndIf}
            System::Call '$SYSDIR\bcrypt.dll::BCryptHashData(p R7, p R9, i r6, i 0) i .r1'
            ${If} $1 <> 0
              StrCpy $5 0
              ${ExitDo}
            ${EndIf}
          ${Loop}
          ${If} $5 = 1
            System::Call '$SYSDIR\bcrypt.dll::BCryptFinishHash(p R7, p R9, i 32, i 0) i .r1'
            ${If} $1 = 0
              ${For} $5 0 31
                IntOp $6 $R9 + $5
                System::Call '*$6(&i1 .r7)'
                IntOp $7 $7 & 0xFF
                IntFmt $7 "%02x" $7
                StrCpy $4 "$4$7"
              ${Next}
              StrCpy $2 1
            ${EndIf}
          ${EndIf}
          System::Free $R9
        ${EndIf}
        System::Call 'kernel32::CloseHandle(p R8)'
      ${EndIf}
      System::Call '$SYSDIR\bcrypt.dll::BCryptDestroyHash(p R7)'
    ${EndIf}
    System::Call '$SYSDIR\bcrypt.dll::BCryptCloseAlgorithmProvider(p R6, i 0)'
  ${EndIf}
  ${If} $2 <> 1
    StrCpy $4 ""
  ${EndIf}
!macroend

; Sets $R4 to 1 when the admin copy may be started elevated, in this order: (1) every level from
; the drive's root to the copy passes CLAUSEO_LEVEL_SAFE, the root against
; CLAUSEO_ROOT_CHANGE_RIGHTS and the rest against CLAUSEO_CHANGE_RIGHTS; (2) the machine's record
; (64-bit view) holds a 64-character AdminCopySha256; (3) the copy's SHA-256
; (CLAUSEO_SHA256_OF_COPY); (4) the two equal, ignoring case. On a mismatch the record is read
; once more and compared again: a refresh may have written a new copy and its hash between the
; first read and the hash. $R4 is 2 when the hash couldn't be computed, else 0 when anything
; fails or can't be read. Keeps $0 to $9, $R0 to $R2 and $R6 to $R9.
!macro CLAUSEO_ADMIN_COPY_VERIFIED
  Push $0
  Push $1
  Push $2
  Push $3
  Push $4
  Push $5
  Push $6
  Push $7
  Push $8
  Push $9
  Push $R0
  Push $R1
  Push $R2
  Push $R6
  Push $R7
  Push $R8
  Push $R9
  StrCpy $R4 0
  ; 1. The chain: each prefix of the copy's path that ends before a backslash, the root (C:\)
  ; first, then the copy itself.
  StrCpy $0 "${CLAUSEO_ADMIN_EXE}"
  StrCpy $5 1
  StrCpy $1 $0 2 1
  ${If} $1 != ":\"
    StrCpy $5 0
  ${EndIf}
  StrLen $2 $0
  ${If} $5 = 1
    ${For} $3 2 $2
      StrCpy $1 $0 1 $3
      ${If} $1 == "\"
      ${OrIf} $3 = $2
        StrCpy $4 $0 $3
        StrCpy $6 ${CLAUSEO_CHANGE_RIGHTS}
        StrCpy $7 16
        ${If} $3 = 2
          StrCpy $4 $0 3
          StrCpy $6 ${CLAUSEO_ROOT_CHANGE_RIGHTS}
        ${ElseIf} $3 = $2
          StrCpy $7 0
        ${EndIf}
        !insertmacro CLAUSEO_LEVEL_SAFE
        ${If} $5 = 0
          ${ExitFor}
        ${EndIf}
      ${EndIf}
    ${Next}
  ${EndIf}
  ${If} $5 = 1
    ; 2. The recorded hash.
    SetRegView 64
    ReadRegStr $3 HKLM "SOFTWARE\Clauseo\Sandbox" "AdminCopySha256"
    SetRegView lastused
    StrLen $2 $3
    ${If} $2 = 64
      ; 3. The copy's hash, then 4. the two compared, the record read again once on a mismatch.
      !insertmacro CLAUSEO_SHA256_OF_COPY
      ${If} $2 <> 1
        StrCpy $R4 2
      ${Else}
        ${If} $4 != $3
          SetRegView 64
          ReadRegStr $3 HKLM "SOFTWARE\Clauseo\Sandbox" "AdminCopySha256"
          SetRegView lastused
        ${EndIf}
        ${If} $4 == $3
          StrCpy $R4 1
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  Pop $R9
  Pop $R8
  Pop $R7
  Pop $R6
  Pop $R2
  Pop $R1
  Pop $R0
  Pop $9
  Pop $8
  Pop $7
  Pop $6
  Pop $5
  Pop $4
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; Sets $R3 to the set-up's owner, the record's OwnerSid, when the uninstaller may say that the
; sandbox's hidden accounts are still on the computer, else "". Not the key alone: --adopt
; --per-machine writes it with nobody set up, and its admin-copy values outlive a removal, which
; deletes OwnerSid. A per-user uninstall says it only to that owner: another Windows user's
; uninstall leaves their set-up alone (CLAUSEO_REMOVE has no --per-machine), so it isn't theirs to
; hear about. The uninstalling user's SID comes from its own token (CLAUSEO_OWN_SID,
; admin_copy.nsh: System::Call, as the removal uses), compared exactly, as the record check does.
; The per-machine uninstall removes every user's set-up, so it says it whoever the owner is. The
; record is in the 64-bit view, which this 32-bit uninstaller reads only through SetRegView 64;
; put back after. Sets $R2; keeps $R4 to $R7.
!macro CLAUSEO_OWNER_TO_TELL
  SetRegView 64
  ReadRegStr $R3 HKLM "${CLAUSEO_RECORD_KEY}" "${CLAUSEO_RECORD_OWNER_SID}"
  SetRegView lastused
  !if "${INSTALLMODE}" == "currentUser"
    ${If} $R3 != ""
      ; CLAUSEO_OWN_SID sets $R2 and uses $R4 to $R7; $R4 and $R5 are read later.
      Push $R4
      Push $R5
      Push $R6
      Push $R7
      !insertmacro CLAUSEO_OWN_SID
      Pop $R7
      Pop $R6
      Pop $R5
      Pop $R4
      ${If} $R2 S!= $R3
        StrCpy $R3 ""
      ${EndIf}
    ${EndIf}
  !endif
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; 0. The app closed first.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"

    ; 1. S15's removal, elevated. ExecShellWait gives no exit code, so ShellExecuteExW with
    ; "runas" (per-user; "open" per-machine) and SEE_MASK_NOCLOSEPROCESS (0x40), then wait on
    ; the process and read its code.
    ; SEE_MASK_FLAG_NO_UI (0x400) keeps Windows' own error box away when the call fails; it does
    ; not suppress the consent prompt. A declined prompt (1223) or a policy that blocks the
    ; program (1260) fails the call, exactly like a failed --remove: $R5 stays 0 and the app
    ; keeps `sandbox\`. $R5 is 1 only when the removal ran and exited 0.
    StrCpy $R5 0
    ; The per-machine installer adds --per-machine (S15 PR4j, stage F). INSTALLMODE is the
    ; template's compile-time define, so the two installers differ here; never read at run time.
    ; Only currentUser and perMachine are built ("both" would pick the mode at run time). The
    ; per-machine uninstaller is elevated already (SYSTEM in session 0 under Intune), so it starts
    ; the tool with "open", which its manifest runs elevated; "runas" is for the per-user one.
    ; Inside the macro because the template !includes hooks.nsh (installer.nsi:36) before it
    ; defines INSTALLMODE (:46): a !if at file scope would see no define.
    !ifndef INSTALLMODE
      !error "hooks.nsh needs the template's INSTALLMODE define"
    !endif
    !if "${INSTALLMODE}" != "perMachine"
      !if "${INSTALLMODE}" != "currentUser"
        !error "hooks.nsh supports only the currentUser and perMachine install modes"
      !endif
    !endif
    !ifndef CLAUSEO_REMOVE
      !if "${INSTALLMODE}" == "perMachine"
        !define CLAUSEO_REMOVE "--remove --uninstall --per-machine"
      !else
        !define CLAUSEO_REMOVE "--remove --uninstall"
      !endif
    !endif
    !ifndef CLAUSEO_REMOVE_VERB
      !if "${INSTALLMODE}" == "perMachine"
        !define CLAUSEO_REMOVE_VERB "open"
      !else
        !define CLAUSEO_REMOVE_VERB "runas"
      !endif
    !endif
    ; The admin-only copy of the set-up tool, in Program Files. The hook starts this copy and never
    ; the one beside the app. NSIS expands $PROGRAMFILES64 where the path is used, at run time.
    !ifndef CLAUSEO_ADMIN_EXE
      !define CLAUSEO_ADMIN_EXE "$PROGRAMFILES64\Clauseo\Sandbox\clauseo-sandbox-setup.exe"
    !endif
    ; A silent per-user uninstall (/S) never raises UAC: it skips the removal and goes on as after
    ; a declined prompt (1223), $R5 left at 0 so `sandbox\` is kept, with one line in the details
    ; (removeSkippedSilent) and no box. IT removes the set-up from an elevated prompt before such
    ; an uninstall, or reinstalls and uninstalls interactively (docs/firm-it.md). The per-machine
    ; uninstaller is elevated already and runs the removal silent or not. $R3 1: run it.
    StrCpy $R3 1
    !if "${INSTALLMODE}" == "currentUser"
      ${If} ${Silent}
      ${AndIf} ${FileExists} "${CLAUSEO_ADMIN_EXE}"
        StrCpy $R3 0
        DetailPrint "$(removeSkippedSilent)"
      ${EndIf}
    !endif
    ${If} $R3 = 1
    ${AndIf} ${FileExists} "${CLAUSEO_ADMIN_EXE}"
      ; The copy checked first (CLAUSEO_ADMIN_COPY_VERIFIED: the chain, then the hash compared
      ; with the record's), and started only when it passes. Refused: nothing starts, $R5 stays
      ; 0 so `sandbox\` is kept, and under /S exit code 27. A hash that couldn't be computed
      ; ($R4 2) says so in the details list. The hidden accounts are said to be still there
      ; (removeAdminCopyRefused, a details line and a box unless /S) only to whom
      ; CLAUSEO_OWNER_TO_TELL names an owner for; to anyone else, a details line that claims
      ; nothing about them, in English, as NSIS's own details lines are, and no box.
      !insertmacro CLAUSEO_ADMIN_COPY_VERIFIED
      ${If} $R4 <> 1
        ${If} $R4 = 2
          DetailPrint "couldn't compute the sandbox tool's hash"
        ${EndIf}
        !insertmacro CLAUSEO_OWNER_TO_TELL
        ${If} $R3 != ""
          DetailPrint "$(removeAdminCopyRefused)"
          ${IfNot} ${Silent}
            MessageBox MB_OK|MB_ICONEXCLAMATION "$(removeAdminCopyRefused)"
          ${EndIf}
        ${ElseIf} $R4 = 0
          DetailPrint "the sandbox tool in Program Files failed its check, so it wasn't started"
        ${EndIf}
        ${If} ${Silent}
          SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_REFUSED}
        ${EndIf}
      ${Else}
      ; The machine's class for an exe, opened read-only (HKEY_LOCAL_MACHINE 0x80000002, KEY_READ
      ; 0x20019) into $R4, is the class key (hkeyClass) the start looks its verb up in, with
      ; SEE_MASK_CLASSKEY (0x3) in fMask: never HKCU\Software\Classes. A key that can't be opened
      ; starts nothing, as a failed start: $R5 stays 0.
      System::Call 'advapi32::RegOpenKeyExW(p 0x80000002, w "SOFTWARE\Classes\exefile", i 0, i 0x20019, *p .R4) i .R2'
      ${If} $R2 = 0
      ; SHELLEXECUTEINFOW as a 32-bit process lays it out: every member 4 bytes, no padding.
      !if ${NSIS_PTR_SIZE} > 4
        !error "hooks.nsh lays out SHELLEXECUTEINFOW for a 32-bit installer; lay it out again"
      !endif
      ; cbSize, fMask, hwnd, lpVerb, lpFile, lpParameters, lpDirectory, nShow (SW_HIDE),
      ; hInstApp, lpIDList, lpClass, hkeyClass, dwHotKey, hIcon, hProcess.
      System::Call '*(&l4, i 0x443, p $HWNDPARENT, w "${CLAUSEO_REMOVE_VERB}", w "${CLAUSEO_ADMIN_EXE}", w "${CLAUSEO_REMOVE}", p 0, i 0, p 0, p 0, p 0, p R4, i 0, p 0, p 0) p .R6'
      System::Call 'shell32::ShellExecuteExW(p R6) i .R7'
      ${If} $R7 <> 0
        System::Call '*$R6(i, i, p, p, p, p, p, i, p, p, p, p, i, p, p .R7)'
        ${If} $R7 <> 0
          StrCpy $R8 1
          ; At most 2 minutes (K3). WAIT_TIMEOUT (258): the tool is still running, so $R8 stays
          ; 1 (failed) and the uninstall carries on. Never TerminateProcess: a removal stopped
          ; midway is worse than one finishing late. Its admin copy may then stay in place.
          System::Call 'kernel32::WaitForSingleObject(p R7, i 120000) i .R3'
          ${If} $R3 = 258
            DetailPrint "$(removeStillRunning)"
          ${ElseIf} $R3 = 0
            ; WAIT_OBJECT_0 (0) only: the tool ended, so its exit code is read, into a value
            ; seeded 1. A failed GetExitCodeProcess (it returns 0) counts as a failed removal, as
            ; do WAIT_FAILED and a timeout, which never read it: $R8 stays non-zero.
            System::Call 'kernel32::GetExitCodeProcess(p R7, *i 1 R8) i .R2'
            ${If} $R2 = 0
              StrCpy $R8 1
            ${EndIf}
          ${EndIf}
          System::Call 'kernel32::CloseHandle(p R7)'
          ${If} $R8 = 0
          ${OrIf} $R8 = ${CLAUSEO_REMOVED_COPY_LEFT}
            StrCpy $R5 1
          ${EndIf}
        ${EndIf}
      ${EndIf}
      System::Free $R6
      System::Call 'advapi32::RegCloseKey(p R4)'
      ${EndIf}
      ${EndIf}
    ${EndIf}
    ; The tool gone (quarantined, say), so nothing was removed: said only to whom
    ; CLAUSEO_OWNER_TO_TELL names a set-up's owner for, whose hidden accounts are then still
    ; there. The tool is the admin copy, the only one the removal above starts. Under /S no box: a
    ; details line and the exit code, which the uninstaller only returns when run in place with
    ; _?= (its stub returns at once).
    ${IfNot} ${FileExists} "${CLAUSEO_ADMIN_EXE}"
      !insertmacro CLAUSEO_OWNER_TO_TELL
      ${If} $R3 != ""
        DetailPrint "$(removeExeMissing)"
        ${If} ${Silent}
          SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}
        ${Else}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(removeExeMissing)"
        ${EndIf}
      ${EndIf}
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
      ; The app's language (the app writes it), so Software\Clauseo goes too. Only here: when the
      ; lawyer keeps their data it stays, so a reinstall's uninstaller speaks their language.
      DeleteRegValue HKCU "${MANUKEY}" "UiLanguage"
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

; The installer's language, both installers: the template's block "ui-language" inserts this in
; .onInit, before NSIS fixes the pages' language, so setting $LANGUAGE here takes effect. The
; lawyer's choice in the app (HKCU\Software\Clauseo\UiLanguage, `en` or `hi`, which the app
; writes) wins over Windows' display language: a repair, a re-run and the updater's /UPDATE
; re-run all follow it. The first install has no value yet, so Windows decides. A per-machine
; install elevated as another account (an administrator's credentials at the UAC prompt) reads
; that account's HKCU, so that account's choice or Windows'; accepted. LANG_HINDI and
; LANG_ENGLISH come from the template's MUI_LANGUAGE lines, before .onInit. $R0 is put back;
; the error flag a failed read sets is read by nothing after this in .onInit.
!macro CLAUSEO_UI_LANGUAGE
  !ifndef LANG_HINDI
    !error "hooks.nsh needs Hindi in bundle.windows.nsis.languages"
  !endif
  Push $R0
  ; A failed read leaves "", which is neither: NSIS's choice is kept.
  ReadRegStr $R0 HKCU "${MANUKEY}" "UiLanguage"
  ${If} $R0 S== "hi"
    StrCpy $LANGUAGE ${LANG_HINDI}
  ${ElseIf} $R0 S== "en"
    StrCpy $LANGUAGE ${LANG_ENGLISH}
  ${EndIf}
  Pop $R0
!macroend

; Repair mode's switch and wait (the paragraph "Repair" above), first in Section Install: the
; template inserts this right before its own CheckIfAppIsRunning. The per-machine installer
; ignores the switch: $ClauseoRepair stays 0 there.
!macro NSIS_HOOK_PREINSTALL
  StrCpy $ClauseoRepair 0
  !if "${INSTALLMODE}" == "currentUser"
    Push $R0
    ${GetOptions} $CMDLINE "${CLAUSEO_REPAIR_SWITCH}" $R0
    ${IfNot} ${Errors}
      StrCpy $ClauseoRepair 1
    ${EndIf}
    Pop $R0
    ${If} $ClauseoRepair = 1
      !insertmacro CLAUSEO_REPAIR_WAIT
    ${EndIf}
  !endif
!macroend

; Waits until Clauseo is gone, at most CLAUSEO_REPAIR_WAIT_MS, looking every
; CLAUSEO_REPAIR_POLL_MS: gone is both no process using $INSTDIR\Clauseo.exe, found as the
; template's CheckIfAppIsRunning finds it (a Restart Manager session with the file registered,
; RmGetList's ERROR_MORE_DATA meaning some process has it), and no top-level window of class
; CLAUSEO_MAIN_WINDOW_CLASS (FindWindow, hidden ones included), which admin_copy.nsh's check
; looks for. A session or a registration that fails leaves only the window to wait for, as the
; template's own check then finds nothing. Past the wait it carries on: what follows decides.
; Keeps every register.
!macro CLAUSEO_REPAIR_WAIT
  Push $0
  Push $1
  Push $2
  Push $3
  Push $R0
  Push $R1
  Push $R2
  Push $R3
  StrCpy $R3 0
  !insertmacro RestartManager_StartSession $R0
  ${If} $R0 != ""
    !insertmacro RestartManager_RegisterFile $R0 "$INSTDIR\${MAINBINARYNAME}.exe"
    ${If} $0 = 0
      StrCpy $R3 1
    ${EndIf}
  ${EndIf}
  StrCpy $R1 0
  ${Do}
    StrCpy $R2 0
    ${If} $R3 = 1
      System::Call 'RSTRTMGR::RmGetList(p R0, *i .r1, *i .r2, p 0, *i .r3) i .r0'
      ${If} $0 = ${ERROR_MORE_DATA}
        StrCpy $R2 1
      ${EndIf}
    ${EndIf}
    FindWindow $1 "${CLAUSEO_MAIN_WINDOW_CLASS}"
    ${If} $1 <> 0
      StrCpy $R2 1
    ${EndIf}
    ${If} $R2 = 0
    ${OrIf} $R1 >= ${CLAUSEO_REPAIR_WAIT_MS}
      ${ExitDo}
    ${EndIf}
    Sleep ${CLAUSEO_REPAIR_POLL_MS}
    IntOp $R1 $R1 + ${CLAUSEO_REPAIR_POLL_MS}
  ${Loop}
  ${If} $R0 != ""
    !insertmacro RestartManager_EndSession $R0
  ${EndIf}
  Pop $R3
  Pop $R2
  Pop $R1
  Pop $R0
  Pop $3
  Pop $2
  Pop $1
  Pop $0
!macroend

; Repair mode's end, last in the per-user NSIS_HOOK_POSTINSTALL, after the admin step whatever its
; outcome: Clauseo started once, unelevated as the user, as the template's RunMainBinary does;
; then the template's passive path, so its SkipIfPassive skips the finish page and its "Run
; Clauseo" box, and the installer closes by itself. Not after a wait for the elevated instance that
; timed out: the usual finish page then, which hides "Run Clauseo" (CLAUSEO_FINISH_SHOW). Outside
; any Push and Pop, as RunAsUser may leave a result on the stack.
!macro CLAUSEO_REPAIR_RELAUNCH
  ${If} $ClauseoRepair = 1
  ${AndIf} $ClauseoAdminTimedOut <> 1
    nsis_tauri_utils::RunAsUser "$INSTDIR\${MAINBINARYNAME}.exe" ""
    StrCpy $PassiveMode 1
    SetAutoClose true
  ${EndIf}
!macroend

; The x64 per-machine installer on a computer whose processor is ARM64 (Windows on ARM runs it, and
; the x64 Clauseo it installs, under emulation): one details line naming the arm64 per-machine
; installer as the computer's native one, so IT can deploy that one next time. It installs as
; always: no prompt, no box, no exit code (a non-zero code reads as a failure in Intune and SCCM),
; and the same under /S. The per-user installer says nothing: the app's updater moves an x64
; per-user install to the arm64 build at its next update. The native machine is
; IsWow64Process2's (kernel32, Windows 10 1709 and later), as crates/sandbox/src/windows/detect.rs
; reads it; where the call is missing or fails the line is left out. IMAGE_FILE_MACHINE_ARM64 is
; 0xAA64. The line is in the installer's language, like the other strings: nativeArm64Notice in
; English.nsh and Hindi.nsh.
!macro CLAUSEO_NATIVE_ARM64_NOTICE
  !ifndef ARCH
    !error "hooks.nsh needs the template's ARCH define"
  !endif
  !if "${ARCH}" == "x64"
    Push $R1
    Push $R2
    Push $R3
    StrCpy $R1 0
    StrCpy $R3 0
    System::Call 'kernel32::GetCurrentProcess() p .R2'
    ; Both USHORTs written into 4-byte buffers that start at 0; a missing export leaves $R1 "error".
    System::Call 'kernel32::IsWow64Process2(p R2, *i 0 .R2, *i 0 .R3) i .R1'
    ${If} $R1 <> 0
    ${AndIf} $R3 = 0xAA64
      DetailPrint "$(nativeArm64Notice)"
    ${EndIf}
    Pop $R3
    Pop $R2
    Pop $R1
  !endif
!macroend

; The firm-policy folder and the admin copy, per-machine installs; the per-user installer asks for
; the admin section. The test is inside the macro: the template (installer.nsi) includes this file
; at line 36, before `!define INSTALLMODE` at line 46, so a test at file scope would see none.
!macro NSIS_HOOK_POSTINSTALL
  ; The uninstaller's language, both installers. LANG_HINDI and LANG_ENGLISH come from the
  ; template's MUI_LANGUAGE lines (installer.nsi:471), after this file is included but before the
  ; section this macro is inserted in.
  !ifndef LANG_HINDI
    !error "hooks.nsh needs Hindi in bundle.windows.nsis.languages"
  !endif
  Push $R0
  Push $R1
  StrCpy $R1 $LANGUAGE
  ; A failed read leaves "", which is neither: the installer's own language is kept.
  ReadRegStr $R0 HKCU "${MANUKEY}" "UiLanguage"
  ${If} $R0 S== "hi"
    StrCpy $R1 ${LANG_HINDI}
  ${ElseIf} $R0 S== "en"
    StrCpy $R1 ${LANG_ENGLISH}
  ${EndIf}
  WriteRegStr HKCU "${MANUPRODUCTKEY}" "Installer Language" $R1
  Pop $R1
  Pop $R0
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
    ; $APPDATA here is C:\ProgramData only because the context is all (set above).
    System::Call 'kernel32::GetFileAttributesW(w "$APPDATA\Clauseo") i .R7'
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
        System::Call 'kernel32::CreateDirectoryW(w "$APPDATA\Clauseo", p R7) i .R8 ?e'
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
    ; The x64 installer on an ARM64 computer names the native one (a details line only).
    !insertmacro CLAUSEO_NATIVE_ARM64_NOTICE
    ; The admin copy, every run (admin_copy.nsh).
    !insertmacro CLAUSEO_ADMIN_COPY_MACHINE
    Pop $R9
    Pop $R8
    Pop $R7
    Pop $R6
    Pop $R5
  !else
    !insertmacro CLAUSEO_ASK_FOR_THE_ADMIN_SECTION
    !insertmacro CLAUSEO_REPAIR_RELAUNCH
  !endif
!macroend

; The per-machine uninstaller's install-folder key. The template's Section Install writes
; HKLM\SOFTWARE\Clauseo\Clauseo (MANUPRODUCTKEY under SHCTX, whose default value is $INSTDIR, in
; the 64-bit view SetContext picks on x64 and arm64). The uninstall deleted it only with the
; "delete app data" box ticked (NSIS_HOOK_PREUNINSTALL, which takes over the template's block for
; the box); unticked, and always under /S, the key outlived the uninstall, naming a deleted
; folder. The template inserts this hook near the end of Section Uninstall, after its own
; removals and before the self-delete (uninstall_in_place.nsh); an uninstall that refuses or
; aborts before then (exit 26, Clauseo running) never gets here. Then HKLM\SOFTWARE\Clauseo, only
; when empty: the sandbox's record (Sandbox) or anything else keeps it. Not under /UPDATE, which
; keeps everything; the template's update mode doesn't run the old uninstaller at all, and a
; reinstall that runs it (the reinstall page, without /UPDATE) writes the key again in its own
; Section Install afterwards. Per user the HKCU key still stays unless the box is ticked, as
; before. The test is inside the macro: INSTALLMODE isn't defined yet where this file is
; included.
!macro NSIS_HOOK_POSTUNINSTALL
  !if "${INSTALLMODE}" == "perMachine"
    ${If} $UpdateMode <> 1
      SetRegView 64
      DeleteRegKey HKLM "${MANUPRODUCTKEY}"
      DeleteRegKey /ifempty HKLM "${MANUKEY}"
      SetRegView lastused
    ${EndIf}
  !endif
!macroend
