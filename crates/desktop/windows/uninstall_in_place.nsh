; The per-machine uninstaller runs in place, then deletes itself (decision 0049, "What stays
; open"), which admin_copy.nsh includes. Per-user stays as Tauri's template has it: its copy in
; %TEMP% runs unelevated as the same user, so it gains nothing.
;
; Why. Without "_?=" on its command line, NSIS's stub copies the uninstaller to
; %TEMP%\~nsu<n>.tmp\Un.exe and starts that copy, elevated, before any script runs (NSIS 3.11
; Main.c:322-420): an elevated image in the user's own %TEMP%, which their code can swap between
; the copy and the start. With "_?=<folder>" the stub runs the uninstaller where it is, no copy.
;
; What.
; - The template's block "uninstall-string" writes, per machine, UninstallString
;   "<folder>\uninstall.exe" _?=<folder> and QuietUninstallString "<folder>\uninstall.exe" /S
;   _?=<folder> (HKLM, which only administrators write); options go before _?=. NSIS takes all
;   that follows the last " _?=" as the folder (Main.c:330-339), so the reinstall page, which
;   appends its own "_?=$4" after the registry's value, still sets the folder it chose, and an
;   option appended after _?= would become part of the folder.
; - So un.onInit (block "un-admin-section", after the protected plug-in folder) refuses to run
;   unless $EXEDIR is $INSTDIR (CLAUSEO_UNINSTALL_IN_PLACE_CHECK): "UninstallString /S" would
;   otherwise unregister Clauseo and leave its files. Exit 26, uninstallNotInPlace (no box under
;   /S). It also refuses the %TEMP% copy that a by-hand start without _?= makes.
; - NSIS holds its own exe open, sharing reads only, until it exits (fileform.c:192, util.c:465,
;   Main.c:472), so the template's Delete "$INSTDIR\uninstall.exe" fails, and its RMDir "$INSTDIR"
;   with it (they name only that file and the folder, non-recursively). At the end of the section
;   (block "uninstall-self-delete") CLAUSEO_UNINSTALL_SELF_DELETE builds a hidden cmd line that,
;   about 3 s later (PING -n 4), once the uninstaller has exited, deletes uninstall.exe if the
;   main exe isn't there, then removes $INSTDIR only when empty (rmdir without /s). By then NSIS's
;   exit cleanup has removed the uninstaller's own plug-in folder, which sits directly in
;   %ProgramFiles%\Clauseo, the default $INSTDIR (safe_temp.nsh). un.onInit has already removed
;   earlier builds' empty installer-temp there (the same macro).
; - The guard: an install writes the main exe (File, first in Section Install) before
;   WriteUninstaller, so with no main exe the uninstall.exe there is still this old one. A new
;   install in the folder meanwhile writes the main exe first, and the line leaves its
;   uninstall.exe. The guard loses only if that install's main exe and WriteUninstaller both land
;   in the instant between cmd's "if" and its "del"; then the new uninstall.exe goes, which a
;   reinstall puts back. When the guard holds, the old file stays, which is harmless.
; - It starts once. Interactive and /P (both have a window) start it from un.onGUIEnd (block
;   "un-gui-end"), after the user closes the window; the section's end would be too early, since
;   the window stays on "Completed" until Close. /S has no window, so no un.onGUIEnd: the section
;   starts it. Starting clears the line, so nothing can start it twice. cmd starts in $SYSDIR
;   (SetOutPath first), so its working folder doesn't hold $INSTDIR open.
; - The reinstall page runs the old uninstaller this same way (in place, never with /UPDATE: the
;   template's update mode skips the uninstall). The self-delete is not skipped there: the guard
;   keeps the new install's uninstall.exe, and a folder removed while the user is still on the
;   folder page is made again by the install's SetOutPath.
;
; It builds no line, and leaves uninstall.exe, when $INSTDIR\<main exe> holds one of
; % ^ & | < > " ! (cmd expands % and, with delayed expansion, !, even in quotes; the rest end the
; quoting or the command), or uninstall.exe isn't there. The file is harmless: firm-it.md says it
; may stay until removed by hand.

; The cmd line, built in the section and started once (empty when there is nothing to start).
Var ClauseoSelfDelete
; The per-machine uninstaller's exit code when it isn't running in place (docs/firm-it.md):
; nothing was uninstalled.
!define CLAUSEO_EXIT_NOT_IN_PLACE 26

; Sets OUT to 1 when PATH holds none of % ^ & | < > " !, else 0. Plain NSIS and LogicLib, one
; character at a time. OUT must not be $R0, $R1 or $R2.
!macro CLAUSEO_CMD_SAFE PATH OUT
  Push $R0
  Push $R1
  Push $R2
  StrCpy $R2 "${PATH}"
  StrCpy ${OUT} 1
  StrCpy $R0 0
  ${Do}
    StrCpy $R1 $R2 1 $R0
    ${If} $R1 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R1 == "%"
    ${OrIf} $R1 == "^"
    ${OrIf} $R1 == "&"
    ${OrIf} $R1 == "|"
    ${OrIf} $R1 == "<"
    ${OrIf} $R1 == ">"
    ${OrIf} $R1 == "$\""
    ${OrIf} $R1 == "!"
      StrCpy ${OUT} 0
      ${ExitDo}
    ${EndIf}
    IntOp $R0 $R0 + 1
  ${Loop}
  Pop $R2
  Pop $R1
  Pop $R0
!macroend

; Starts the cmd line, once: hidden, by its full path, in $SYSDIR, then forgets it.
!macro CLAUSEO_START_SELF_DELETE
  ${If} $ClauseoSelfDelete != ""
    SetOutPath $SYSDIR
    ExecShell "open" "$SYSDIR\cmd.exe" '$ClauseoSelfDelete' SW_HIDE
    StrCpy $ClauseoSelfDelete ""
  ${EndIf}
!macroend

; In un.onInit (block "un-admin-section"), after the protected folder: refuses unless $EXEDIR is
; $INSTDIR, compared without case (StrCmp) and with one trailing "\" taken off each. Keeps $0-$2.
!macro CLAUSEO_UNINSTALL_IN_PLACE_CHECK
  !if "${INSTALLMODE}" == "perMachine"
    Push $0
    Push $1
    Push $2
    StrCpy $0 "$EXEDIR"
    StrCpy $2 $0 1 -1
    ${If} $2 == "\"
      StrCpy $0 $0 -1
    ${EndIf}
    StrCpy $1 "$INSTDIR"
    StrCpy $2 $1 1 -1
    ${If} $2 == "\"
      StrCpy $1 $1 -1
    ${EndIf}
    ${If} $0 != $1
      MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST "$(uninstallNotInPlace)" /SD IDOK
      SetErrorLevel ${CLAUSEO_EXIT_NOT_IN_PLACE}
      Quit
    ${EndIf}
    Pop $2
    Pop $1
    Pop $0
  !endif
!macroend

; Last in Section Uninstall (block "uninstall-self-delete"), after everything else is removed.
; Keeps $0.
!macro CLAUSEO_UNINSTALL_SELF_DELETE
  !if "${INSTALLMODE}" == "perMachine"
    Push $0
    StrCpy $ClauseoSelfDelete ""
    ; The longest path the line names.
    !insertmacro CLAUSEO_CMD_SAFE "$INSTDIR\${MAINBINARYNAME}.exe" $0
    ${If} $0 = 1
    ${AndIf} ${FileExists} "$INSTDIR\uninstall.exe"
      ; /d: an elevated cmd would otherwise run HKCU's Command Processor AutoRun, the user's own
      ; hive under split-token UAC. /v:off: no ! expansion, whatever HKCU says. The outer quotes:
      ; with more than two quotes, cmd /c drops the first and the last. The brackets end the if
      ; before the rmdir (a quoted ")" in a path doesn't close them).
      StrCpy $ClauseoSelfDelete '/d /v:off /c ""$SYSDIR\PING.EXE" -n 4 127.0.0.1 >nul & (if not exist "$INSTDIR\${MAINBINARYNAME}.exe" del /f /q "$INSTDIR\uninstall.exe") & rmdir "$INSTDIR""'
    ${EndIf}
    Pop $0
    ; /S: no window, so no un.onGUIEnd; start it now. Interactive and /P wait for un.onGUIEnd.
    ${If} ${Silent}
      !insertmacro CLAUSEO_START_SELF_DELETE
    ${EndIf}
  !endif
!macroend

; In un.onGUIEnd (block "un-gui-end"), which only a run with a window reaches. In every build:
; the per-user one never sets the line, so it starts nothing.
!macro CLAUSEO_UNINSTALL_GUI_END
  !insertmacro CLAUSEO_START_SELF_DELETE
!macroend
