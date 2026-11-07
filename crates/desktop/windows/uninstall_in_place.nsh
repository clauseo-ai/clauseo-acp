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
;   _?=<folder> (HKLM, which only administrators write). NSIS takes the last " _?=" on the line
;   (Main.c, the uninstaller's scan runs backwards from the end), so the reinstall page, which
;   appends its own "_?=$4" after the registry's value, still sets the folder it chose.
; - A running image can't be deleted, so the template's Delete "$INSTDIR\uninstall.exe" fails, and
;   its RMDir "$INSTDIR" with it (it names only that file and the folder, non-recursively). At the
;   end of the section (block "uninstall-self-delete") CLAUSEO_UNINSTALL_SELF_DELETE renames the
;   running image to $INSTDIR\uninstall.<unique>.old and builds a hidden cmd line that, about 3 s
;   later (PING -n 4), deletes that one file and removes $INSTDIR\installer-temp and $INSTDIR, each
;   only when empty (rmdir without /s). By then the uninstaller has exited, and NSIS's exit
;   cleanup has removed its own plug-in folder inside installer-temp (safe_temp.nsh).
; - It starts once. Interactive and /P (both have a window) start it from un.onGUIEnd (block
;   "un-gui-end"), after the user closes the window; the section's end would be too early, since
;   the window stays on "Completed" until Close. /S has no window, so no un.onGUIEnd: the section
;   starts it. Starting clears the line, so nothing can start it twice.
; - The reinstall page runs the old uninstaller this same way (in place, never with /UPDATE: the
;   template's update mode skips the uninstall). The self-delete is not skipped there: it deletes
;   only the renamed .old (a name no installer writes) and folders that are empty; the new install
;   writes a fresh uninstall.exe, and a folder removed while the user is still on the folder page
;   is made again by the install's SetOutPath.
;
; It leaves the file, and starts nothing, when the cmd line can't be built safely: $INSTDIR or the
; renamed path holds one of % ^ & | < > " ! (cmd expands % and, with delayed expansion, !, even in
; quotes; the rest end the quoting or the command), uninstall.exe isn't there (a copy from %TEMP%
; ran, and the template's Delete removed the file), GetTempFileName fails, or the rename fails.
; The file is harmless: firm-it.md says it may stay until removed by hand.

; The cmd line, built in the section and started once (empty when there is nothing to start).
Var ClauseoSelfDelete

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

; Starts the cmd line, once: hidden, by its full path, then forgets it.
!macro CLAUSEO_START_SELF_DELETE
  ${If} $ClauseoSelfDelete != ""
    ExecShell "open" "$SYSDIR\cmd.exe" '$ClauseoSelfDelete' SW_HIDE
    StrCpy $ClauseoSelfDelete ""
  ${EndIf}
!macroend

; Last in Section Uninstall (block "uninstall-self-delete"), after everything else is removed.
; Keeps $0, $1 and $2.
!macro CLAUSEO_UNINSTALL_SELF_DELETE
  !if "${INSTALLMODE}" == "perMachine"
    Push $0
    Push $1
    Push $2
    StrCpy $ClauseoSelfDelete ""
    !insertmacro CLAUSEO_CMD_SAFE $INSTDIR $0
    ${If} $0 = 1
    ${AndIf} ${FileExists} "$INSTDIR\uninstall.exe"
      ClearErrors
      GetTempFileName $1 "$INSTDIR"
      ${IfNot} ${Errors}
        ; GetTempFileName made the file to claim the name, and Rename refuses a target that
        ; exists, so the placeholder goes first. The gap is safe: a per-machine $INSTDIR is
        ; admin-only (Program Files'), so no one else can put anything at that name meanwhile.
        Delete "$1"
        ; Its stem: the name after the last "\", less ".tmp".
        StrLen $2 $1
        ${Do}
          IntOp $2 $2 - 1
          ${If} $2 < 0
            ${ExitDo}
          ${EndIf}
          StrCpy $0 $1 1 $2
          ${If} $0 == "\"
            ${ExitDo}
          ${EndIf}
        ${Loop}
        IntOp $2 $2 + 1
        StrCpy $1 $1 "" $2
        StrCpy $1 $1 -4
        StrCpy $1 "$INSTDIR\uninstall.$1.old"
        !insertmacro CLAUSEO_CMD_SAFE $1 $0
        ${If} $0 = 1
          ClearErrors
          Rename "$INSTDIR\uninstall.exe" "$1"
          ${IfNot} ${Errors}
            ; /d: an elevated cmd would otherwise run HKCU's Command Processor AutoRun, the user's
            ; own hive under split-token UAC. /v:off: no ! expansion, whatever HKCU says. The
            ; outer quotes: with more than two quotes, cmd /c drops the first and the last.
            StrCpy $ClauseoSelfDelete '/d /v:off /c ""$SYSDIR\PING.EXE" -n 4 127.0.0.1 >nul & del /f /q "$1" & rmdir "$INSTDIR\installer-temp" & rmdir "$INSTDIR""'
          ${EndIf}
        ${EndIf}
      ${EndIf}
    ${EndIf}
    Pop $2
    Pop $1
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
