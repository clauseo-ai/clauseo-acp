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
; What. A fresh folder under CLAUSEO_INSTALLER_TEMP, which inherits Program Files' rules: only
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
;   a file still in use goes at the next restart (DEL_REBOOT). So no RMDir of our own is needed.
;
; Ownership. NSIS can't read an owner or a DACL without a plug-in, so neither is checked. The
; parent is the proof: no one but an administrator can create anything under Program Files, so no
; one else can have made the folder, or a link at Clauseo or installer-temp, before us, and an
; administrator can already write anywhere. That matters because CreateDirectory doesn't fail on a
; folder that is already there (exec.c:389-398); icacls then sets the owner and DACL anyway.
;
; No sweep of old folders in CLAUSEO_INSTALLER_TEMP: two installs may run at once, and one would
; delete the other's plug-ins. A folder stays only when an installer is killed. (RMDir /r follows
; junctions; none can be under Program Files but an administrator's.)
;
; Every per-machine run, SYSTEM's (Intune, /S) included: NSIS can't read its own token without a
; plug-in, and what it could read natively, the environment (USERNAME, TEMP), is the user's to set,
; so it can't tell SYSTEM from a lawyer. For SYSTEM the folder is harmless.
;
; It fails closed: nothing is extracted or loaded. The per-machine installer says installTempFailed
; (a box, none under /S) and quits with 25; the per-user elevated instance quits with 99 (not run),
; which its outer instance reports as a failed set-up.

!define CLAUSEO_INSTALLER_TEMP "$PROGRAMFILES64\Clauseo\installer-temp"
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
  StrCpy $1 1
  ClearErrors
  CreateDirectory "${CLAUSEO_INSTALLER_TEMP}"
  GetTempFileName $0 "${CLAUSEO_INSTALLER_TEMP}"
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
      MessageBox MB_OK|MB_ICONSTOP "$(installTempFailed)" /SD IDOK
      SetErrorLevel ${CLAUSEO_EXIT_TEMP_FAILED}
    !else
      SetErrorLevel ${CLAUSEO_ADMIN_NOT_RUN}
    !endif
    Quit
  ${EndIf}
  Pop $1
  Pop $0
!macroend

; The per-machine installer's start, first in .onInit (template block "admin-section"): the
; folder, then the environment's TEMP and TMP set to it (GetTempPath reads TMP first), so the
; programs it starts elevated use it too: the WebView2 bootstrapper, --adopt, and the uninstaller
; of the copy already installed, which the template's reinstall page runs in place (_?=). The
; System calls are this run's first plug-in, loaded from the folder just made.
!macro CLAUSEO_MACHINE_START
  !if "${INSTALLMODE}" == "perMachine"
    !insertmacro CLAUSEO_SAFE_PLUGINSDIR
    System::Call 'kernel32::SetEnvironmentVariableW(w "TEMP", w "$TEMP")'
    System::Call 'kernel32::SetEnvironmentVariableW(w "TMP", w "$TEMP")'
  !endif
!macroend
