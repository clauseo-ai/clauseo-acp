; The per-user installer's elevated instance (admin_copy.nsh's header): CLAUSEO_ELEVATED_BRANCH,
; which the repo's copy of Tauri's template (windows/nsis/installer.nsi, block "admin-section")
; inserts first in .onInit. With /CLAUSEO_ADMIN_SECTION as a whole argument it does the admin
; section and quits there; without it, it changes nothing and .onInit goes on as upstream's.
;
; It runs before anything else of the template's: no plug-in (nothing is loaded from a plug-in
; folder, which NSIS makes under %TEMP%, a folder of the lawyer's when the lawyer approved the
; prompt), no page, no section. So everything here is plain NSIS instructions and LogicLib (which
; expands to plain jumps): no `::` plug-in call, no `Call`, no library function such as
; GetOptions, StrLoc or VersionCompare. A test holds that. Should anything here ever load, it would
; load from the branch's own temporary folder under Program Files (CLAUSEO_SAFE_PLUGINSDIR,
; safe_temp.nsh), which it makes once it has the switch and Clauseo isn't running, and which sets
; $PLUGINSDIR and $TEMP; that macro's two UnsafeStrCpy lines are the only $TEMP the test allows.
;
; It reads nothing the lawyer can write before it extracts: not the environment or HKCU, and
; nothing from %TEMP%. Before .onInit, though, the NSIS stub itself creates the %TEMP% folder,
; which comes from the user's environment (so HKCU\Environment's TEMP picks the path), makes one
; nsX????.tmp in it and deletes it, to test that it can be written (NSIS 3.11 Main.c:71-80,
; 259-284). That leaves an admin-owned empty folder at a path the lawyer's code chose, and a
; one-shot race on the delete; no way to run code through it is known, the per-machine installer
; has the same exposure, and no script can prevent it (decision 0049, "What stays open"). With
; /SOLID dropped (template block "no-solid"), `File` reads the copy straight from the installer,
; with nothing unpacked into %TEMP% first (fileform.c:311-320 does that only for solid data), and
; the branch makes no plug-in folder.
; Its inputs are its own command line, which the outer, unelevated instance wrote, and HKLM
; (administrators only):
; - $INSTDIR, from /D= only (NSIS sets it before .onInit and cuts $CMDLINE there);
; - the owner's SID and data folder, from /CLAUSEO_OWNER_SID= and /CLAUSEO_DATA_DIR=;
; - that SID's profile folder, from HKLM's ProfileList, and the tier-B record.
; The SID must be one; $INSTDIR and the data folder must be absolute paths inside that profile,
; with no "..", quote, slash, wildcard or stream colon, and no trailing backslash. --adopt then
; checks the SID against the outer instance's token (the process chain) and the folders against
; that user's profile again; these checks only keep its command line whole and refuse a bad one
; before anything is written. A refused argument runs nothing (99).

; $R0 1 when /CLAUSEO_ADMIN_SECTION is one of the arguments, whole; $R2 the owner SID and $R3 the
; data folder from their options ("" when absent). The data folder's option comes last and takes
; the rest of the line, spaces included (NSIS has cut $CMDLINE before " /D="). Uses $R1 and $R4 to
; $R7.
!macro CLAUSEO_PARSE_ARGUMENTS
  StrCpy $R0 0
  StrCpy $R2 ""
  StrCpy $R3 ""
  StrCpy $R1 $CMDLINE
  ; Past the program's path: to its closing quote, or to the first space.
  StrCpy $R5 $R1 1
  ${If} $R5 == '"'
    StrCpy $R6 1
    StrCpy $R7 '"'
  ${Else}
    StrCpy $R6 0
    StrCpy $R7 " "
  ${EndIf}
  ${Do}
    StrCpy $R5 $R1 1 $R6
    ${If} $R5 == ""
    ${OrIf} $R5 == $R7
      ${ExitDo}
    ${EndIf}
    IntOp $R6 $R6 + 1
  ${Loop}
  IntOp $R6 $R6 + 1
  StrCpy $R1 $R1 "" $R6
  ; The arguments, one at a time, split at spaces.
  ${Do}
    ${Do}
      StrCpy $R5 $R1 1
      ${If} $R5 != " "
        ${ExitDo}
      ${EndIf}
      StrCpy $R1 $R1 "" 1
    ${Loop}
    ${If} $R1 == ""
      ${ExitDo}
    ${EndIf}
    StrLen $R6 "${CLAUSEO_DATA_DIR_OPTION}"
    StrCpy $R5 $R1 $R6
    ${If} $R5 S== "${CLAUSEO_DATA_DIR_OPTION}"
      StrCpy $R3 $R1 "" $R6
      ${Do}
        StrCpy $R5 $R3 1 -1
        ${If} $R5 != " "
          ${ExitDo}
        ${EndIf}
        StrCpy $R3 $R3 -1
      ${Loop}
      ${ExitDo}
    ${EndIf}
    StrCpy $R6 0
    ${Do}
      StrCpy $R5 $R1 1 $R6
      ${If} $R5 == ""
      ${OrIf} $R5 == " "
        ${ExitDo}
      ${EndIf}
      IntOp $R6 $R6 + 1
    ${Loop}
    StrCpy $R4 $R1 $R6
    StrCpy $R1 $R1 "" $R6
    ${If} $R4 S== "${CLAUSEO_ADMIN_SWITCH}"
      StrCpy $R0 1
    ${Else}
      StrLen $R6 "${CLAUSEO_OWNER_SID_OPTION}"
      StrCpy $R5 $R4 $R6
      ${If} $R5 S== "${CLAUSEO_OWNER_SID_OPTION}"
        StrCpy $R2 $R4 "" $R6
      ${EndIf}
    ${EndIf}
  ${Loop}
!macroend

; $R0 set to 0 unless VALUE is a SID string: "S-1-", then only digits and dashes, at most 184
; characters (a SID's longest), so it also names a ProfileList key and no other. A character is a
; digit when IntOp gives it back unchanged. Uses $R5 to $R7.
!macro CLAUSEO_CHECK_SID VALUE
  StrCpy $R5 ${VALUE} 4
  StrLen $R6 ${VALUE}
  ${IfNot} $R5 S== "S-1-"
  ${OrIf} $R6 > 184
    StrCpy $R0 0
  ${EndIf}
  StrCpy $R6 4
  ${Do}
    StrCpy $R5 ${VALUE} 1 $R6
    ${If} $R5 == ""
      ${ExitDo}
    ${EndIf}
    IntOp $R7 $R5 + 0
    ${If} $R5 != "-"
    ${AndIf} $R7 S!= $R5
      StrCpy $R0 0
    ${EndIf}
    IntOp $R6 $R6 + 1
  ${Loop}
!macroend

; $R0 set to 0 unless $R4, a profile folder from ProfileList, is a plain drive path ("X:\..."),
; with no "%" (an unexpanded variable, which would need the environment) and no quote. Uses $R5
; and $R6.
!macro CLAUSEO_CHECK_PROFILE
  StrCpy $R5 $R4 2 1
  ${If} $R5 != ":\"
    StrCpy $R0 0
  ${EndIf}
  StrCpy $R6 0
  ${Do}
    StrCpy $R5 $R4 1 $R6
    ${If} $R5 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R5 == "%"
    ${OrIf} $R5 == '"'
      StrCpy $R0 0
    ${EndIf}
    IntOp $R6 $R6 + 1
  ${Loop}
!macroend

; $R0 set to 0 unless VALUE is inside the profile folder in $R4 ("<profile>\" starts it, any
; case), with no ".." anywhere, no quote, slash, wildcard, "<", ">" or "|", no ":" but the
; drive's, and no trailing backslash (which would escape the closing quote of --adopt's
; argument). Uses $R5, $R6 and $R9.
!macro CLAUSEO_CHECK_PATH VALUE
  StrLen $R6 "$R4\"
  StrCpy $R5 ${VALUE} $R6
  ${If} $R4 == ""
  ${OrIf} $R5 != "$R4\"
    StrCpy $R0 0
  ${EndIf}
  StrCpy $R5 ${VALUE} 1 -1
  ${If} $R5 == "\"
    StrCpy $R0 0
  ${EndIf}
  StrCpy $R6 0
  StrCpy $R9 ""
  ${Do}
    StrCpy $R5 ${VALUE} 1 $R6
    ${If} $R5 == ""
      ${ExitDo}
    ${EndIf}
    ${If} $R5 == '"'
    ${OrIf} $R5 == "/"
    ${OrIf} $R5 == "*"
    ${OrIf} $R5 == "?"
    ${OrIf} $R5 == "<"
    ${OrIf} $R5 == ">"
    ${OrIf} $R5 == "|"
      StrCpy $R0 0
    ${EndIf}
    ${If} $R5 == ":"
    ${AndIf} $R6 <> 1
      StrCpy $R0 0
    ${EndIf}
    ${If} $R5 == "."
    ${AndIf} $R9 == "."
      StrCpy $R0 0
    ${EndIf}
    StrCpy $R9 $R5
    IntOp $R6 $R6 + 1
  ${Loop}
!macroend

; The branch (header). Per-user installer only; the per-machine one never takes the switch. With
; the switch: the record checked, then the copy written and --adopt run (CLAUSEO_ADMIN_COPY), and
; the instance quits with 100 + its exit code (98 newer copy kept, 99 not run). Quit in .onInit
; ends the installer at once, no callback run, with SetErrorLevel's code (NSIS Ui.c:396,
; Main.c:464). Without it, the registers are put back.
!macro CLAUSEO_ELEVATED_BRANCH
  !if "${INSTALLMODE}" == "currentUser"
    Push $R0
    Push $R1
    Push $R2
    Push $R3
    Push $R4
    Push $R5
    Push $R6
    Push $R7
    !insertmacro CLAUSEO_PARSE_ARGUMENTS
    ${If} $R0 = 1
      ; The 64-bit registry, as the template's SetContext would set it later in .onInit (its
      ; ${RunningX64} test calls a plug-in; these installers are x64 and arm64 only).
      SetRegView 64
      ; Clauseo running (its main window, hidden or minimised included): touch nothing (97). A
      ; correctness guard, not a security boundary: it runs once this exe is already open. A
      ; process of the lawyer's could fake the window and so block this step; that only denies it.
      FindWindow $R5 "${CLAUSEO_MAIN_WINDOW_CLASS}"
      ${If} $R5 <> 0
        SetErrorLevel ${CLAUSEO_ADMIN_APP_RUNNING}
        Quit
      ${EndIf}
      ; Its own temporary folder under Program Files (safe_temp.nsh), so anything that ever loads
      ; here loads from there, never from the lawyer's %TEMP%; it quits with 99 when it can't.
      !insertmacro CLAUSEO_SAFE_PLUGINSDIR
      !insertmacro CLAUSEO_CHECK_SID $R2
      StrCpy $R4 ""
      ${If} $R0 = 1
        ReadRegStr $R4 HKLM "${CLAUSEO_PROFILE_LIST}\$R2" "ProfileImagePath"
        !insertmacro CLAUSEO_CHECK_PROFILE
      ${EndIf}
      !insertmacro CLAUSEO_CHECK_PATH $INSTDIR
      !insertmacro CLAUSEO_CHECK_PATH $R3
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
      SetErrorLevel $R8
      Quit
    ${EndIf}
    Pop $R7
    Pop $R6
    Pop $R5
    Pop $R4
    Pop $R3
    Pop $R2
    Pop $R1
    Pop $R0
  !endif
!macroend
