; The elevated installers' own temporary folder (decision 0049, "What stays open"), which
; admin_copy.nsh includes. CLAUSEO_SAFE_PLUGINSDIR runs first in every elevated path, before any
; plug-in call: in the per-user installer's elevated instance (CLAUSEO_ELEVATED_BRANCH, which
; loads no plug-in, so this is for anything it might ever load) and at the start of the whole
; per-machine installer (CLAUSEO_MACHINE_START). The template's block "admin-section" inserts both
; first in .onInit; NSIS runs no script code before .onInit (Ui.c:396 is the first callback).
;
; Why. NSIS extracts and loads every plug-in from $PLUGINSDIR, which the first plug-in call makes
; in $TEMP (makensis build.cpp:3806-3837: GetTempFileName in $TEMP, Delete, CreateDirectory). $TEMP
; is the process's %TEMP%: an administrator who approves a UAC prompt keeps their own environment,
; so the folder is the lawyer's, writable by every process of theirs at medium integrity, and
; HKCU\Environment picks the path. NSIS restricts the plug-in folder itself (exec.c:380,
; util.c:59-73), but its parent lets the lawyer's code rename it away and put a folder of its own,
; with its own DLL, at the same name before the elevated LoadLibrary.
;
; What. A fresh folder directly in CLAUSEO_TEMP_PARENT (%ProgramFiles%\Clauseo), which inherits
; Program Files' rules: only
; administrators, SYSTEM and TrustedInstaller can create, rename or delete anything there. Its name
; comes from GetTempFileName (Win32 creates the file exclusively, so the name was free); the file
; is deleted and a folder made in its place. icacls ($SYSDIR's, by full path; a console program, so
; a console window may flash) then makes Administrators its owner and gives it a protected DACL,
; Administrators and SYSTEM full control only, and each exit code must be 0. Then $PLUGINSDIR and
; $TEMP are that folder:
; - a $PLUGINSDIR that isn't empty makes NSIS's plug-in initializer skip making its own (its
;   first test, "StrCmp $PLUGINSDIR "" done", build.cpp:3806-3807): every plug-in loads from here;
; - $TEMP covers the run's other $TEMP uses, such as the template's WebView2 bootstrapper, which it
;   writes to $TEMP and runs elevated;
; - at exit NSIS deletes $PLUGINSDIR, recursively, and nothing else (CleanUp, Main.c:470-489),
;   on every way out, Quit and Abort in .onInit included (Ui.c:396 returns, then Main.c:427-429);
;   a file still in use goes at the next restart (DEL_REBOOT). So no RMDir of our own is needed,
;   and nothing of the run stays: there is no parent of its own to outlive it.
;   %ProgramFiles%\Clauseo is where the admin copy lives (Sandbox, admin_copy.nsh) and the
;   per-machine $INSTDIR by default, so the folder can sit inside $INSTDIR. Nothing there removes
;   what it didn't put: the uninstaller deletes its own files by name, and $INSTDIR only when
;   empty, never recursively (the template's section; then its delayed rmdir,
;   uninstall_in_place.nsh, which runs once its own exit cleanup is done), and so does the old
;   uninstaller that an install over a running one starts.
;
; Earlier builds made the folders in a parent of their own, %ProgramFiles%\Clauseo\installer-temp,
; which stayed, empty, after every run. Each run removes it first, without /r (RMDir fails on a
; folder that isn't empty), so an older installer running at the same time keeps its folder.
;
; Ownership. NSIS can't read an owner or a DACL without a plug-in, so neither is checked. The
; parent is the proof: no one but an administrator can create anything under Program Files, so no
; one else can have made the folder, or a link at Clauseo, before us, and an administrator can
; already write anywhere. That matters because CreateDirectory doesn't fail on a folder that is
; already there (exec.c:389-398); icacls then sets the owner and DACL anyway.
;
; No sweep of old folders in CLAUSEO_TEMP_PARENT: two installs may run at once, and one would
; delete the other's plug-ins. A folder stays only when an installer is killed. (RMDir /r follows
; junctions; none can be under Program Files but an administrator's.)
;
; Every per-machine run, SYSTEM's (Intune, /S) included: NSIS can't read its own token without a
; plug-in, and what it could read natively, the environment (USERNAME, TEMP), is the user's to set,
; so it can't tell SYSTEM from a lawyer. For SYSTEM the folder is harmless.
;
; It fails closed: nothing is extracted or loaded. The per-machine installer says installTempFailed
; (a box, none under /S, /P or /UPDATE) and quits with 25, and its uninstaller says
; uninstallTempFailed the same way, with 25 too (nothing uninstalled); the per-user elevated
; instance quits with 99 (not run), which its outer instance reports as a failed set-up.

!define CLAUSEO_TEMP_PARENT "$PROGRAMFILES64\Clauseo"
; Earlier builds' parent (header), removed only when empty.
!define CLAUSEO_OLD_TEMP_PARENT "$PROGRAMFILES64\Clauseo\installer-temp"
; BUILTIN\Administrators and LocalSystem, as icacls takes a SID.
!define CLAUSEO_SID_ADMINISTRATORS "*S-1-5-32-544"
!define CLAUSEO_SID_SYSTEM "*S-1-5-18"
; The per-machine installer's exit code when it can't make the folder (docs/firm-it.md): nothing
; was installed.
!define CLAUSEO_EXIT_TEMP_FAILED 25

; The folder (header). Plain NSIS and LogicLib, as the elevated branch needs: no plug-in, no Call.
; Keeps $0 and $1.
!macro CLAUSEO_SAFE_PLUGINSDIR
  Push $0
  Push $1
  RMDir "${CLAUSEO_OLD_TEMP_PARENT}"
  StrCpy $1 1
  ClearErrors
  CreateDirectory "${CLAUSEO_TEMP_PARENT}"
  GetTempFileName $0 "${CLAUSEO_TEMP_PARENT}"
  ${IfNot} ${Errors}
    Delete "$0"
    CreateDirectory "$0"
    ${IfNot} ${Errors}
      ExecWait '"$SYSDIR\icacls.exe" "$0" /setowner ${CLAUSEO_SID_ADMINISTRATORS} /Q' $1
      ${If} ${Errors}
        StrCpy $1 1
      ${ElseIf} $1 = 0
        ExecWait '"$SYSDIR\icacls.exe" "$0" /inheritance:r /grant:r ${CLAUSEO_SID_ADMINISTRATORS}:(OI)(CI)F ${CLAUSEO_SID_SYSTEM}:(OI)(CI)F /Q' $1
        ${If} ${Errors}
          StrCpy $1 1
        ${EndIf}
      ${EndIf}
    ${EndIf}
  ${EndIf}
  ${If} $1 = 0
    UnsafeStrCpy $PLUGINSDIR "$0"
    UnsafeStrCpy $TEMP "$0"
  ${Else}
    ; The folder, or the file GetTempFileName left, when either is there; never recursive.
    ${If} $0 != ""
      Delete "$0"
      RMDir "$0"
    ${EndIf}
    !if "${INSTALLMODE}" == "perMachine"
      ; No box under /S (/SD), /P or /UPDATE, which the template hasn't parsed yet here. The
      ; uninstaller (un.onInit's block, __UNINSTALL__) says it in its own words.
      !insertmacro CLAUSEO_PASSIVE_OR_UPDATE $1
      ${If} $1 = 0
        !ifdef __UNINSTALL__
          MessageBox MB_OK|MB_ICONSTOP "$(uninstallTempFailed)" /SD IDOK
        !else
          MessageBox MB_OK|MB_ICONSTOP "$(installTempFailed)" /SD IDOK
        !endif
      ${EndIf}
      SetErrorLevel ${CLAUSEO_EXIT_TEMP_FAILED}
    !else
      SetErrorLevel ${CLAUSEO_ADMIN_NOT_RUN}
    !endif
    Quit
  ${EndIf}
  Pop $1
  Pop $0
!macroend

; Sets OUT to 1 when $CMDLINE holds /P or /UPDATE as a whole word (any case, as GetOptions reads
; them), else 0. Plain NSIS and LogicLib (GetOptions is a Call). OUT must not be $2 or $3. A path
; in quotes holding " /P " counts too; that only skips a box.
!macro CLAUSEO_PASSIVE_OR_UPDATE OUT
  Push $2
  Push $3
  StrCpy ${OUT} 0
  StrCpy $2 0
  ${Do}
    StrCpy $3 $CMDLINE 1 $2
    ${If} $3 == ""
      ${ExitDo}
    ${EndIf}
    StrCpy $3 $CMDLINE 3 $2
    ${If} $3 == " /P"
      IntOp $3 $2 + 3
      StrCpy $3 $CMDLINE 1 $3
      ${If} $3 == ""
      ${OrIf} $3 == " "
        StrCpy ${OUT} 1
      ${EndIf}
    ${EndIf}
    StrCpy $3 $CMDLINE 8 $2
    ${If} $3 == " /UPDATE"
      IntOp $3 $2 + 8
      StrCpy $3 $CMDLINE 1 $3
      ${If} $3 == ""
      ${OrIf} $3 == " "
        StrCpy ${OUT} 1
      ${EndIf}
    ${EndIf}
    ${If} ${OUT} = 1
      ${ExitDo}
    ${EndIf}
    IntOp $2 $2 + 1
  ${Loop}
  Pop $3
  Pop $2
!macroend

; The per-machine installer's start, first in .onInit (template block "admin-section"): the
; folder, then the environment's TEMP and TMP set to it (GetTempPath reads TMP first), so the
; programs it starts elevated use it too: the WebView2 bootstrapper, --adopt, and the uninstaller
; of the copy already installed, which the template's reinstall page runs in place (_?=). The
; System calls are this run's first plug-in, loaded from the folder just made. The per-machine
; uninstaller runs the same start first in un.onInit (template block "un-admin-section"): it runs
; elevated in place (_?=, uninstall_in_place.nsh), and SetContext (${RunningX64}, System) and
; CheckIfAppIsRunning (nsis_tauri_utils) would otherwise load from the user's %TEMP%; its children
; (--remove, --delete-app-data) get the folder as TEMP too.
!macro CLAUSEO_MACHINE_START
  !if "${INSTALLMODE}" == "perMachine"
    !insertmacro CLAUSEO_SAFE_PLUGINSDIR
    System::Call 'kernel32::SetEnvironmentVariableW(w "TEMP", w "$TEMP")'
    System::Call 'kernel32::SetEnvironmentVariableW(w "TMP", w "$TEMP")'
  !endif
!macroend
