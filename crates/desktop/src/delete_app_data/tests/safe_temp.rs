//! The elevated installers' own temporary folder (`windows/safe_temp.nsh`): made fresh under
//! Program Files, owned by Administrators and locked to Administrators and SYSTEM, then set as
//! `$PLUGINSDIR` and `$TEMP`, first in every elevated path and before any plug-in call: in the
//! per-user installer's elevated branch and at the start of the per-machine installer's
//! `.onInit`. The Windows VM checks the folder and its DACL for real.

use super::admin_copy::{admin_code, macro_body};
use super::sources::{line_with, read};
use super::template::{TEMPLATE, reached, split_blocks};

/// A line that loads a plug-in, itself or through what it inserts: a `::` call, x64.nsh's
/// `${RunningX64}` (System), the template's `SetContext` (which tests it), the language picker
/// (`MUI_LANGDLL_DISPLAY`), `MULTIUSER_INIT`, Tauri's `SemverCompare`, and any `Call` (whose
/// function might).
pub(super) fn loads_a_plug_in(line: &str) -> bool {
    line.contains("::")
        || line.contains("${RunningX64}")
        || line.contains("SemverCompare")
        || line.starts_with("!insertmacro SetContext")
        || line.starts_with("!insertmacro MUI_LANGDLL_DISPLAY")
        || line.starts_with("!insertmacro MULTIUSER_INIT")
        || line.starts_with("Call ")
}

#[test]
fn the_plug_in_test_knows_the_templates_plug_in_lines() {
    for line in [
        "nsis_tauri_utils::SemverCompare \"${VERSION}\" $R0",
        "${If} ${RunningX64}",
        "!insertmacro SetContext",
        "!insertmacro MUI_LANGDLL_DISPLAY",
        "System::Call 'kernel32::AttachConsole(i -1)i.r0'",
    ] {
        assert!(loads_a_plug_in(line), "{line}");
    }
    assert!(!loads_a_plug_in("!insertmacro CLAUSEO_SAFE_PLUGINSDIR"));
}

#[test]
fn the_per_machine_installer_makes_its_folder_before_its_first_plug_in() {
    // NSIS runs no script code before .onInit (Ui.c:396), so .onInit's first plug-in is the run's.
    let text = read(TEMPLATE);
    let lines: Vec<&str> = text.split('\n').map(str::trim).collect();
    let start = lines
        .iter()
        .position(|line| *line == "Function .onInit")
        .unwrap();
    let end = start
        + lines[start..]
            .iter()
            .position(|line| *line == "FunctionEnd")
            .unwrap();
    let on_init = &lines[start..end];
    let block_end = on_init
        .iter()
        .position(|line| *line == "; <<< clauseo: admin-section")
        .unwrap();
    let first_plug_in = on_init
        .iter()
        .position(|line| !line.starts_with(';') && loads_a_plug_in(line))
        .unwrap();
    assert!(block_end < first_plug_in, "{:?}", on_init[first_plug_in]);
    // The block (pinned in template.rs) inserts the per-user branch, then the per-machine start.
    let (blocks, _) = split_blocks(&text);
    let block = blocks
        .iter()
        .find(|block| block.name == "admin-section")
        .unwrap();
    let branch = line_with(&block.lines, &["!insertmacro CLAUSEO_ELEVATED_BRANCH"]);
    let machine = line_with(&block.lines, &["!insertmacro CLAUSEO_MACHINE_START"]);
    assert!(branch < machine);
    // The branch is empty in the per-machine build, so the start is the first thing it runs.
    let code = admin_code();
    let body = macro_body(&code, "CLAUSEO_ELEVATED_BRANCH");
    assert_eq!(body[1], "!if \"${INSTALLMODE}\" == \"currentUser\"");
    assert_eq!(body[body.len() - 1], "!endif");
    assert_eq!(
        macro_body(&code, "CLAUSEO_MACHINE_START")[1..],
        [
            "!if \"${INSTALLMODE}\" == \"perMachine\"",
            "!insertmacro CLAUSEO_SAFE_PLUGINSDIR",
            "System::Call 'kernel32::SetEnvironmentVariableW(w \"TEMP\", w \"$TEMP\")'",
            "System::Call 'kernel32::SetEnvironmentVariableW(w \"TMP\", w \"$TEMP\")'",
            "!endif",
        ]
    );
    // And the folder's own macro loads nothing.
    for (name, body) in reached(&code, "CLAUSEO_SAFE_PLUGINSDIR") {
        for line in body {
            assert!(!loads_a_plug_in(line), "{name}: {line}");
        }
    }
}

#[test]
fn the_per_user_elevated_branch_makes_its_folder_before_it_writes_or_runs_anything() {
    let code = admin_code();
    let body = macro_body(&code, "CLAUSEO_ELEVATED_BRANCH");
    let folder = line_with(body, &["!insertmacro CLAUSEO_SAFE_PLUGINSDIR"]);
    // After the switch and the running check (which touches nothing), before the record, the
    // extraction and --adopt.
    assert!(line_with(body, &["FindWindow "]) < folder);
    assert!(folder < line_with(body, &["!insertmacro CLAUSEO_RECORD_CHECK"]));
    assert!(folder < line_with(body, &["!insertmacro CLAUSEO_ADMIN_COPY "]));
    // Nothing the branch reaches loads a plug-in, before the folder or after it.
    for (name, body) in reached(&code, "CLAUSEO_ELEVATED_BRANCH") {
        for line in body {
            assert!(!loads_a_plug_in(line), "{name}: {line}");
        }
    }
}

#[test]
fn the_folder_is_made_fresh_under_program_files_owned_locked_and_then_used() {
    let code = admin_code();
    line_with(
        &code,
        &["!define CLAUSEO_INSTALLER_TEMP \"$PROGRAMFILES64\\Clauseo\\installer-temp\""],
    );
    line_with(
        &code,
        &["!define CLAUSEO_SID_ADMINISTRATORS \"*S-1-5-32-544\""],
    );
    line_with(&code, &["!define CLAUSEO_SID_SYSTEM \"*S-1-5-18\""]);
    line_with(&code, &["!define CLAUSEO_EXIT_TEMP_FAILED 25"]);
    // Every step, in order: a name GetTempFileName made free, a folder in its place, icacls'
    // owner and protected DACL (each exit code 0, a start that failed counted as a failure), then
    // $PLUGINSDIR and $TEMP. Any failure removes what it made, never recursively, and quits.
    assert_eq!(
        macro_body(&code, "CLAUSEO_SAFE_PLUGINSDIR")[1..],
        [
            "Push $0",
            "Push $1",
            "StrCpy $1 1",
            "ClearErrors",
            "CreateDirectory \"${CLAUSEO_INSTALLER_TEMP}\"",
            "GetTempFileName $0 \"${CLAUSEO_INSTALLER_TEMP}\"",
            "${IfNot} ${Errors}",
            "Delete \"$0\"",
            "CreateDirectory \"$0\"",
            "${IfNot} ${Errors}",
            "ExecWait '\"$SYSDIR\\icacls.exe\" \"$0\" /setowner ${CLAUSEO_SID_ADMINISTRATORS} /Q' $1",
            "${If} ${Errors}",
            "StrCpy $1 1",
            "${ElseIf} $1 = 0",
            "ExecWait '\"$SYSDIR\\icacls.exe\" \"$0\" /inheritance:r /grant:r \
             ${CLAUSEO_SID_ADMINISTRATORS}:(OI)(CI)F ${CLAUSEO_SID_SYSTEM}:(OI)(CI)F /Q' $1",
            "${If} ${Errors}",
            "StrCpy $1 1",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
            "${If} $1 = 0",
            "UnsafeStrCpy $PLUGINSDIR \"$0\"",
            "UnsafeStrCpy $TEMP \"$0\"",
            "${Else}",
            "${If} $0 != \"\"",
            "Delete \"$0\"",
            "RMDir \"$0\"",
            "${EndIf}",
            "!if \"${INSTALLMODE}\" == \"perMachine\"",
            "!insertmacro CLAUSEO_PASSIVE_OR_UPDATE $1",
            "${If} $1 = 0",
            "!ifdef __UNINSTALL__",
            "MessageBox MB_OK|MB_ICONSTOP \"$(uninstallTempFailed)\" /SD IDOK",
            "!else",
            "MessageBox MB_OK|MB_ICONSTOP \"$(installTempFailed)\" /SD IDOK",
            "!endif",
            "${EndIf}",
            "SetErrorLevel ${CLAUSEO_EXIT_TEMP_FAILED}",
            "!else",
            "SetErrorLevel ${CLAUSEO_ADMIN_NOT_RUN}",
            "!endif",
            "Quit",
            "${EndIf}",
            "Pop $1",
            "Pop $0",
        ]
    );
    // The words the per-machine installer shows, and the include, before the branch's.
    let english = read("windows/nsis/English.nsh");
    assert!(english.contains("LangString installTempFailed ${LANG_ENGLISH} \""));
    assert!(english.contains("LangString uninstallTempFailed ${LANG_ENGLISH} \""));
    let copy = read("windows/admin_copy.nsh");
    let temp = copy
        .find("!include \"${__FILEDIR__}\\safe_temp.nsh\"")
        .unwrap();
    let branch = copy
        .find("!include \"${__FILEDIR__}\\admin_elevated.nsh\"")
        .unwrap();
    assert!(temp < branch);
}

#[test]
fn the_box_is_skipped_under_passive_and_update_by_a_native_scan_of_the_command_line() {
    let code = admin_code();
    // /P and /UPDATE as whole words, any case (StrCmp), each followed by a space or the end; no
    // Call (GetOptions is one), so the per-user elevated branch can reach it.
    assert_eq!(
        macro_body(&code, "CLAUSEO_PASSIVE_OR_UPDATE")[1..],
        [
            "Push $2",
            "Push $3",
            "StrCpy ${OUT} 0",
            "StrCpy $2 0",
            "${Do}",
            "StrCpy $3 $CMDLINE 1 $2",
            "${If} $3 == \"\"",
            "${ExitDo}",
            "${EndIf}",
            "StrCpy $3 $CMDLINE 3 $2",
            "${If} $3 == \" /P\"",
            "IntOp $3 $2 + 3",
            "StrCpy $3 $CMDLINE 1 $3",
            "${If} $3 == \"\"",
            "${OrIf} $3 == \" \"",
            "StrCpy ${OUT} 1",
            "${EndIf}",
            "${EndIf}",
            "StrCpy $3 $CMDLINE 8 $2",
            "${If} $3 == \" /UPDATE\"",
            "IntOp $3 $2 + 8",
            "StrCpy $3 $CMDLINE 1 $3",
            "${If} $3 == \"\"",
            "${OrIf} $3 == \" \"",
            "StrCpy ${OUT} 1",
            "${EndIf}",
            "${EndIf}",
            "${If} ${OUT} = 1",
            "${ExitDo}",
            "${EndIf}",
            "IntOp $2 $2 + 1",
            "${Loop}",
            "Pop $3",
            "Pop $2",
        ]
    );
    // The box only when neither is there; 25 either way, set after the box's test.
    let temp = macro_body(&code, "CLAUSEO_SAFE_PLUGINSDIR");
    let scan = line_with(temp, &["!insertmacro CLAUSEO_PASSIVE_OR_UPDATE $1"]);
    assert_eq!(temp[scan + 1], "${If} $1 = 0");
    assert!(scan < line_with(temp, &["MessageBox "]));
    assert!(line_with(temp, &["MessageBox "]) < line_with(temp, &["SetErrorLevel ${CLAUSEO_EXIT"]));
}
