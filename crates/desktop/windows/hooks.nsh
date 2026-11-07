; Clauseo's hooks into Tauri's NSIS installer (bundle.windows.nsis.installerHooks). Only the
; release overlay crates/desktop/release/windows.conf.json names this file, so it is in the
; Windows installers that release.yml builds and in nothing else. The template (tauri-cli 2.12.2,
; installer.nsi) inserts NSIS_HOOK_PREUNINSTALL first in `Section Uninstall` (lines 783-787),
; before its own CheckIfAppIsRunning (788) and its "delete app data" block (877-891), and
; NSIS_HOOK_PREINSTALL first in `Section Install` (648-650), after only `SetOutPath $INSTDIR`
; and before its CheckIfAppIsRunning (652).
;
; NSIS_HOOK_PREUNINSTALL runs only for a real uninstall: Tauri's installer runs the old
; uninstaller with /UPDATE during an update ($UpdateMode = 1), which keeps everything.
;
; 0. The app must be closed before anything else: the hook runs the template's own
;    CheckIfAppIsRunning (utils.nsh, which installer.nsi includes at line 26) first, so declining
;    its prompt aborts the uninstall before S15's removal or any deletion. The template's own call
;    at 788 then finds nothing running.
; 1. When the sandbox's set-up tool (S15, clauseo-sandbox-setup.exe) is installed, the
;    uninstaller asks it to remove what it set up, elevated, so a currentUser uninstall raises one
;    UAC prompt. The installer places an admin-only copy in `%ProgramFiles%\Clauseo\Sandbox\`, and
;    the hook starts that copy only: a per-user copy beside the app is never started elevated. The
;    tool reads its records under the data folder's `sandbox` folder to take its access rules back
;    off the lawyer's folders. sidecars.mjs bundles it in the Windows installers.
;    Before starting it the hook checks the copy as the app does (admin_copy.rs): every folder from
;    the drive's root down to it, and the copy, admin-only; then the copy's SHA-256 (certutil) the
;    one the machine's record holds (AdminCopySha256). Anything else, or anything it can't read,
;    and it refuses: nothing is started, `sandbox\` is kept, and it says so
;    (removeAdminCopyRefused): a box, or under /S a line in the details list and exit code
;    CLAUSEO_EXIT_ADMIN_COPY_REFUSED.
;    The start looks its verb up in HKLM\SOFTWARE\Classes\exefile alone (SEE_MASK_CLASSKEY),
;    never in HKCU\Software\Classes, which the uninstalling user can write.
;    Both pass `--uninstall`, which the app's in-app removal never does: it also takes the admin
;    copy, its record values and the refresh task away, last, for the install that placed them.
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
;    removed. A removal that didn't finish (any exit but 0) keeps `sandbox\` and the machine's
;    record; the recovery is to reinstall Clauseo, then uninstall it again (decision 0049).
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
; setupExeRemoved, and the admin copy's adminCopyNotSetUp, adminCopyOtherAccount,
; adminCopySkippedSilent and adminCopyStillRunning); the uninstall-declined line is S15a's.
;
; NSIS_HOOK_POSTINSTALL, the template's last step of `Section Install`: the per-machine installer
; creates $APPDATA\Clauseo (C:\ProgramData\Clauseo, where a firm's policy.json goes; see
; docs/firm-it.md section 6) owned by Administrators, so a user who made the folder first cannot
; make a firm's policy be ignored. $APPDATA is the all-users folder only because the hook sets the
; context to all around its work (NSIS has no COMMONAPPDATA; the old name was taken as text).
; A folder that is already there is left as it is.
; The uninstaller leaves that folder, since a firm's policy may be in it.
; Then both installers check that clauseo-sandbox-setup.exe is still in $INSTDIR: Windows security
; can remove it as it is written. Gone, they say so (setupExeRemoved): a box, or under /S a line
; in the details list and exit code CLAUSEO_EXIT_SETUP_EXE_REMOVED. The install stays.
;
; The language (English or Hindi, windows/nsis/*.nsh). NSIS fixes the pages' language after
; .onInit and un.onInit (NSIS 3.11 Source/exehead/Ui.c:396-397, set_language; Docs/src/var.but:54
; "You can only change this variable in .onInit"), and the template gives neither a hook, so
; MUI_CUSTOMFUNCTION_GUIINIT (Ui.c:525, after set_language) would be too late. The installer
; therefore speaks Windows' display language (NSIS's own choice), English otherwise. The
; uninstaller's un.onInit runs MUI_UNGETLANGUAGE (installer.nsi:770), which reads
; HKCU\${MANUPRODUCTKEY} "Installer Language" (installer.nsi:164-166) and, when that is empty and
; the uninstall is not silent, shows LangDLL's language picker (two languages). So
; NSIS_HOOK_POSTINSTALL writes that value at every install and update: the language the lawyer
; chose in the app (HKCU\Software\Clauseo\UiLanguage, `en` or `hi`, which the app writes), else
; the installer's own. The template deletes it with the data (installer.nsi:884, and step 2).
;
; The admin copy (S15 tier B): elevated code runs the sandbox's set-up program only from
; CLAUSEO_ADMIN_DIR (%ProgramFiles%\Clauseo\Sandbox), which only administrators can write, never
; from $INSTDIR, which a per-user install leaves writable by the lawyer's processes. The copy
; comes from this installer's own payload, before Clauseo or any agent has run:
; - Per-user, NSIS_HOOK_POSTINSTALL (interactive, not /UPDATE) starts $EXEPATH again with
;   "runas" and `/S /UPDATE ${CLAUSEO_ADMIN_SWITCH} /D=$INSTDIR`: one UAC prompt. NSIS holds
;   $EXEPATH open sharing only reads until it exits, so nothing can rewrite, rename or delete it
;   meanwhile.
; - In that elevated instance NSIS_HOOK_PREINSTALL, Section Install's first step, sees the switch,
;   writes the program from the payload into the folder and runs `--adopt` there
;   (CLAUSEO_ADMIN_COPY), then quits with 100 + its exit code, before the template's
;   CheckIfAppIsRunning or any File of its own. Before that hook, the template's .onInit only
;   reads switches and the registry (it has no single-instance or app-running check), and /UPDATE
;   makes Section WebView2 skip installing WebView2, which the first instance did.
; - The first instance shows a box for a declined prompt or any code but done (adminCopyNotSetUp,
;   adminCopyOtherAccount). The install has succeeded either way, with tier C.
; - /S and /P can't ask: a details line, and exit code 22 for /S. /UPDATE (Tauri's updater) skips
;   it without a word; S15 PR 3's task refreshes the copy. Running the installer again by hand
;   runs it again (--adopt is idempotent).
; - The per-machine installer is elevated already: its NSIS_HOOK_POSTINSTALL writes the copy and
;   runs `--adopt --per-machine` itself, every time, updates included.

; NSIS's warning 6000 is "unknown variable/constant detected, ignoring" (Source/build.h,
; DW_VAR_IGNORED_UNKNOWN): an unknown name stays as literal text and the hook does the wrong thing
; quietly. This makes that warning an error for the whole compile (the template includes this file
; before its own code). The source test the_installer_scripts_use_only_known_nsis_variables checks
; the names as well.
!pragma warning error 6000 ; an unknown variable name is an error, not a silent literal (the COMMONAPPDATA bug)

; Exit codes of `Clauseo.exe --delete-app-data` (crates/desktop/src/delete_app_data.rs; a test
; holds them equal).
!define CLAUSEO_DELETED 0
!define CLAUSEO_KEPT_SANDBOX 2
!define CLAUSEO_KEPT_SET_ASIDE 3
!define CLAUSEO_KEPT_AND_FAILED 4

; The installers' own exit codes under /S (SetErrorLevel), listed in docs/firm-it.md, "If Windows
; security removed Clauseo's set-up program". Tauri's template sets none, and NSIS uses 0, 1
; (cancelled) and 2 (aborted); these stay clear of those and of the codes Intune and SCCM read as
; success or retry (1223, 1618, 1641, 1707, 3010).
!define CLAUSEO_EXIT_SETUP_EXE_REMOVED 20
!define CLAUSEO_EXIT_REMOVE_EXE_MISSING 21
; 22 to 26 are kept for the installers' admin-copy steps.
!define CLAUSEO_EXIT_ADMIN_COPY_REFUSED 27

; The rights admin_copy.rs's chain rule counts: CHANGE_RIGHTS on every level below the drive's
; root, ROOT_CHANGE_RIGHTS on the root (a test holds them equal); and TrustedInstaller's SID,
; which may own or change a level, as SYSTEM (S-1-5-18) and Administrators (S-1-5-32-544) may.
!define CLAUSEO_CHANGE_RIGHTS 0x520D0146
!define CLAUSEO_ROOT_CHANGE_RIGHTS 0x120C0040
!define CLAUSEO_TRUSTED_INSTALLER "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464"

; The admin copy. The switch makes an instance of the per-user installer the admin section only.
; $PROGRAMFILES64 is the 64-bit Program Files, from this 32-bit installer. CLAUSEO_HOOKS_DIR is
; this file's folder, for `File`: sidecars.mjs writes the program to crates/desktop/binaries.
!define CLAUSEO_ADMIN_SWITCH "/CLAUSEO_ADMIN_SECTION"
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
; The elevated instance exits with 100 + --adopt's code, so NSIS's own 1 (cancelled) and 2
; (aborted, as by .onInit's downgrade check) never read as --adopt's; with 99 when the program
; could not be written or started.
!define CLAUSEO_ADMIN_EXIT_BASE 100
!define CLAUSEO_ADMIN_NOT_RUN 99
; The installers' own exit codes under /S (SetErrorLevel), clear of NSIS's 0 to 2 and of #638's
; 20 and 21 (packaging-setup-exe-missing). 22: a silent per-user install, which can't ask for the
; UAC prompt, skipped the admin copy. 23: the per-machine installer's --adopt failed.
!define CLAUSEO_EXIT_ADMIN_COPY_SKIPPED 22
!define CLAUSEO_EXIT_ADMIN_COPY_FAILED 23

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

; certutil's SHA-256 of the copy at $0, written to the file at $4 in the uninstaller's own
; temporary folder ($PLUGINSDIR): certutil started directly (CreateProcessW, no window, never
; through cmd.exe), its output and errors the file, and waited for up to a minute. $2 is 1 when it
; ended in time. The file is created anew, inheritable (SECURITY_ATTRIBUTES: nLength 12, no
; descriptor, bInheritHandle 1), for writing (GENERIC_WRITE), shared for reading, CREATE_ALWAYS
; (2), FILE_ATTRIBUTE_NORMAL (0x80). A 32-bit STARTUPINFOW is 68 bytes, with STARTF_USESHOWWINDOW
; | STARTF_USESTDHANDLES (0x101), SW_HIDE and the handles last; a PROCESS_INFORMATION 16
; (hProcess, hThread, ...). Uses $1 and $R6 to $R9.
!macro CLAUSEO_CERTUTIL_HASH
  StrCpy $2 0
  System::Call '*(i 12, p 0, i 1) p .R6'
  System::Call 'kernel32::CreateFileW(w r4, i 0x40000000, i 1, p R6, i 2, i 0x80, p 0) p .R7'
  System::Free $R6
  ${If} $R7 <> -1
    StrCpy $1 '"$SYSDIR\certutil.exe" -hashfile "$0" SHA256'
    System::Call '*(i 68, p 0, p 0, p 0, i 0, i 0, i 0, i 0, i 0, i 0, i 0, i 0x101, &i2 0, &i2 0, p 0, p 0, p R7, p R7) p .R8'
    System::Call '*(&i16) p .R9'
    ; bInheritHandles 1, for the file; CREATE_NO_WINDOW (0x08000000).
    System::Call 'kernel32::CreateProcessW(w "$SYSDIR\certutil.exe", w r1, p 0, p 0, i 1, i 0x08000000, p 0, p 0, p R8, p R9) i .r1'
    System::Call 'kernel32::CloseHandle(p R7)'
    ${If} $1 <> 0
      System::Call '*$R9(p .R6, p .R7)'
      System::Call 'kernel32::CloseHandle(p R7)'
      System::Call 'kernel32::WaitForSingleObject(p R6, i 60000) i .r1'
      ${If} $1 = 0
        StrCpy $2 1
      ${EndIf}
      System::Call 'kernel32::CloseHandle(p R6)'
    ${EndIf}
    System::Free $R8
    System::Free $R9
  ${EndIf}
!macroend

; Sets $R4 to 1 when a line of certutil's output in the file at $4, spaces, tabs, CR and LF
; dropped (Windows 7's certutil spaces the bytes), is the recorded hash in $3, compared as NSIS
; compares (case-insensitively). The output is ANSI text, or UTF-16LE (a NUL or a byte order
; mark's 0xFE as its second byte). Uses $1, $8, $9 and $R6 to $R8.
!macro CLAUSEO_HASH_LINE_MATCHES
  ClearErrors
  FileOpen $R6 "$4" r
  ${IfNot} ${Errors}
    FileReadByte $R6 $1
    FileReadByte $R6 $1
    StrCpy $R7 0
    ${If} $1 = 0
    ${OrIf} $1 = 254
      StrCpy $R7 1
    ${EndIf}
    FileSeek $R6 0 SET
    ${Do}
      ClearErrors
      ${If} $R7 = 1
        FileReadUTF16LE $R6 $1
      ${Else}
        FileRead $R6 $1
      ${EndIf}
      ${If} ${Errors}
        ${ExitDo}
      ${EndIf}
      StrCpy $8 ""
      StrCpy $9 0
      ${Do}
        StrCpy $R8 $1 1 $9
        ${If} $R8 == ""
          ${ExitDo}
        ${EndIf}
        ${If} $R8 != " "
        ${AndIf} $R8 != "$\t"
        ${AndIf} $R8 != "$\r"
        ${AndIf} $R8 != "$\n"
          StrCpy $8 "$8$R8"
        ${EndIf}
        IntOp $9 $9 + 1
      ${Loop}
      ${If} $8 == $3
        StrCpy $R4 1
      ${EndIf}
    ${Loop}
    FileClose $R6
  ${EndIf}
!macroend

; Sets $R4 to 1 when the admin copy may be started elevated, in this order: (1) every level from
; the drive's root to the copy passes CLAUSEO_LEVEL_SAFE, the root against
; CLAUSEO_ROOT_CHANGE_RIGHTS and the rest against CLAUSEO_CHANGE_RIGHTS; (2) the machine's record
; (64-bit view) holds a 64-character AdminCopySha256; (3) certutil hashes the copy; (4) a line of
; its output is that hash. Anything that fails or can't be read leaves $R4 0. Keeps $0 to $9,
; $R0 to $R2 and $R6 to $R9.
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
      ; 3. The copy's hash, then 4. the two compared.
      InitPluginsDir
      StrCpy $4 "$PLUGINSDIR\admin-copy-sha256.txt"
      !insertmacro CLAUSEO_CERTUTIL_HASH
      ${If} $2 = 1
        !insertmacro CLAUSEO_HASH_LINE_MATCHES
      ${EndIf}
      Delete "$4"
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
      ; 0 so `sandbox\` is kept, a details line, and a box or under /S the exit code.
      !insertmacro CLAUSEO_ADMIN_COPY_VERIFIED
      ${If} $R4 <> 1
        DetailPrint "$(removeAdminCopyRefused)"
        ${If} ${Silent}
          SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_REFUSED}
        ${Else}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(removeAdminCopyRefused)"
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
            StrCpy $R5 1
          ${EndIf}
        ${EndIf}
      ${EndIf}
      System::Free $R6
      System::Call 'advapi32::RegCloseKey(p R4)'
      ${EndIf}
      ${EndIf}
    ${EndIf}
    ; The tool gone (quarantined, say), so nothing was removed: said only when the machine's
    ; record exists, the protection having been set up. The tool is the admin copy, the only one
    ; the removal above starts. The record is in the 64-bit view, which this 32-bit uninstaller
    ; reads only through SetRegView 64; put back after. EnumRegValue sets the error flag when the
    ; key can't be opened. Under /S no box: a details line and the exit code, which the
    ; uninstaller only returns when run with _?= (its stub returns at once).
    ${IfNot} ${FileExists} "${CLAUSEO_ADMIN_EXE}"
      SetRegView 64
      ClearErrors
      EnumRegValue $R3 HKLM "SOFTWARE\Clauseo\Sandbox" 0
      ${IfNot} ${Errors}
        DetailPrint "$(removeExeMissing)"
        ${If} ${Silent}
          SetErrorLevel ${CLAUSEO_EXIT_REMOVE_EXE_MISSING}
        ${Else}
          MessageBox MB_OK|MB_ICONEXCLAMATION "$(removeExeMissing)"
        ${EndIf}
      ${EndIf}
      SetRegView lastused
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

; The admin copy (header, "The admin copy"): writes the set-up program from this installer's
; payload into CLAUSEO_ADMIN_DIR and runs `--adopt ARGS` from there with ExecWait (the owner
; --adopt records is this process's user; never through cmd.exe). $R8: its exit code, or
; CLAUSEO_ADMIN_NOT_RUN when the folder or the file could not be written or the program not
; started. Only an elevated process can write there: the folder inherits Program Files' rules,
; and --adopt then locks it (protected, owner Administrators). Never a copy of $INSTDIR's program
; nor anything through $TEMP, either of which could be swapped before it runs. SetOverwrite try:
; a file that can't be replaced (running, say) is skipped with the error flag set, never NSIS's
; Abort/Retry/Ignore box, and --adopt doesn't run. Inside the macro because ARCH is defined after
; the template includes this file (installer.nsi:36, :56).
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

; The elevated instance of the per-user installer (header, "The admin copy"): with the switch it
; is the admin section only and quits here, before the rest of Section Install. Without it, the
; install goes on as the template has it. The per-machine installer never takes the switch.
!macro NSIS_HOOK_PREINSTALL
  !if "${INSTALLMODE}" == "currentUser"
    Push $R0
    ${GetOptions} $CMDLINE "${CLAUSEO_ADMIN_SWITCH}" $R0
    ${IfNot} ${Errors}
      ; $LOCALAPPDATA in the current user's context (the template's SetContext): the elevated
      ; account's own. Under another account's credentials the install folder is not under its
      ; profile, and --adopt refuses (1) before changing anything.
      !insertmacro CLAUSEO_ADMIN_COPY '--user-install-dir "$INSTDIR" --data-dir "$LOCALAPPDATA\${BUNDLEID}"'
      ${If} $R8 <> ${CLAUSEO_ADMIN_NOT_RUN}
        IntOp $R8 $R8 + ${CLAUSEO_ADMIN_EXIT_BASE}
      ${EndIf}
      ; Quit runs no callback (.onInstSuccess, .onInstFailed): the exit code is this one.
      SetErrorLevel $R8
      Quit
    ${EndIf}
    Pop $R0
  !endif
!macroend

; The per-user installer asks for the admin section (header, "The admin copy"), at the end of a
; successful install. $R6 is the outcome: 0 set up, 1 not (a box), 2 another Windows account has
; it (a box), 3 still running after 5 minutes (a details line; it finishes by itself).
!macro CLAUSEO_ASK_FOR_THE_ADMIN_SECTION
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
    ; ShellExecuteExW "runas" on this installer, as the uninstall hook starts --remove:
    ; SEE_MASK_NOCLOSEPROCESS (0x40) for the exit code, SEE_MASK_FLAG_NO_UI (0x400) for no error
    ; box of Windows' own (the consent prompt still shows), SEE_MASK_NOZONECHECKS (0x800000) so a
    ; downloaded installer the person already ran is not warned about again. /D= goes last (NSIS
    ; reads the rest of the line as the folder). A declined prompt (1223) or a blocked program
    ; (1260) fails the call: not set up. The working folder is $SYSDIR, nothing the lawyer writes.
    !if ${NSIS_PTR_SIZE} > 4
      !error "hooks.nsh lays out SHELLEXECUTEINFOW for a 32-bit installer; lay it out again"
    !endif
    System::Call '*(&l4, i 0x800440, p $HWNDPARENT, w "runas", w "$EXEPATH", w "/S /UPDATE ${CLAUSEO_ADMIN_SWITCH} /D=$INSTDIR", w "$SYSDIR", i 0, p 0, p 0, p 0, p 0, i 0, p 0, p 0) p .R5'
    System::Call 'shell32::ShellExecuteExW(p R5) i .R7'
    ${If} $R7 <> 0
      System::Call '*$R5(i, i, p, p, p, p, p, i, p, p, p, p, i, p, p .R7)'
      ${If} $R7 <> 0
        ; At most 5 minutes; past that (WAIT_TIMEOUT, 258) the instance carries on by itself and
        ; is never stopped. Only an ended wait (WAIT_OBJECT_0, 0) reads its code, seeded "not run".
        System::Call 'kernel32::WaitForSingleObject(p R7, i 300000) i .R9'
        ${If} $R9 = 258
          StrCpy $R6 3
        ${ElseIf} $R9 = 0
          System::Call 'kernel32::GetExitCodeProcess(p R7, *i ${CLAUSEO_ADMIN_NOT_RUN} R8) i .R9'
          ${If} $R9 = 0
            StrCpy $R8 ${CLAUSEO_ADMIN_NOT_RUN}
          ${EndIf}
        ${EndIf}
        System::Call 'kernel32::CloseHandle(p R7)'
      ${EndIf}
    ${EndIf}
    System::Free $R5
    ${If} $R6 <> 3
      ; --adopt's own code from the instance's 100 + code; -1 when --adopt never ran (99, or
      ; NSIS's own 1 or 2). Every code is named: done, another account, and the rest not set up.
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
        ${CaseElse}
          StrCpy $R6 1
      ${EndSelect}
    ${EndIf}
    ${If} $R6 = 1
      DetailPrint "$(adminCopyNotSetUp)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyNotSetUp)"
    ${ElseIf} $R6 = 2
      DetailPrint "$(adminCopyOtherAccount)"
      MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyOtherAccount)"
    ${ElseIf} $R6 = 3
      DetailPrint "$(adminCopyStillRunning)"
    ${EndIf}
  ${EndIf}
  Pop $R9
  Pop $R8
  Pop $R7
  Pop $R6
  Pop $R5
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
    ; The admin copy, every run (header, "The admin copy"). $INSTDIR must be
    ; %ProgramFiles%\Clauseo for --per-machine; elsewhere --adopt refuses (1): not set up.
    !insertmacro CLAUSEO_ADMIN_COPY '--per-machine --user-install-dir "$INSTDIR"'
    SetOutPath "$INSTDIR"
    ${If} $R8 <> ${CLAUSEO_ADOPT_DONE}
      DetailPrint "$(adminCopyNotSetUp)"
      ${If} ${Silent}
        SetErrorLevel ${CLAUSEO_EXIT_ADMIN_COPY_FAILED}
      ${Else}
        MessageBox MB_OK|MB_ICONEXCLAMATION "$(adminCopyNotSetUp)"
      ${EndIf}
    ${EndIf}
    Pop $R9
    Pop $R8
    Pop $R7
    Pop $R6
    Pop $R5
  !else
    !insertmacro CLAUSEO_ASK_FOR_THE_ADMIN_SECTION
  !endif
  ; Both installers: the sandbox's set-up program still there. Windows security can quarantine it
  ; as it is written; the install stays as it is, and the person or IT is told. Under /S no box:
  ; a details line and the exit code, for Intune or SCCM.
  ${IfNot} ${FileExists} "$INSTDIR\clauseo-sandbox-setup.exe"
    DetailPrint "$(setupExeRemoved)"
    ${If} ${Silent}
      SetErrorLevel ${CLAUSEO_EXIT_SETUP_EXE_REMOVED}
    ${Else}
      MessageBox MB_ICONEXCLAMATION|MB_OK "$(setupExeRemoved)"
    ${EndIf}
  ${EndIf}
!macroend
