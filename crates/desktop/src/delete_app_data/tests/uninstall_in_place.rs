//! The per-machine uninstaller runs in place and deletes itself (`windows/uninstall_in_place.nsh`):
//! `_?=` in both registry commands, its plug-in folder first in `un.onInit`, the running image
//! renamed and then deleted by a delayed hidden cmd line that removes only empty folders, started
//! once per run. Per-user keeps Tauri's lines. The Windows VM runs it for real.

use super::admin_copy::{assert_run, code_of, macro_body};
use super::safe_temp::loads_a_plug_in;
use super::sources::{line_with, read};
use super::template::{TEMPLATE, block_code, split_blocks};

fn code() -> Vec<String> {
    code_of("windows/uninstall_in_place.nsh")
}

/// The template's trimmed lines from the one equal to `first` to the next `last`.
fn span(text: &str, first: &str, last: &str) -> Vec<String> {
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| line.trim().to_owned())
        .collect();
    let start = lines.iter().position(|line| line == first).unwrap();
    let end = start + lines[start..].iter().position(|line| line == last).unwrap();
    lines[start..=end].to_vec()
}

#[test]
fn per_machine_registers_both_commands_in_place_and_per_user_keeps_tauris_line() {
    let (blocks, _) = split_blocks(&read(TEMPLATE));
    assert_eq!(
        block_code(&blocks, "uninstall-string"),
        [
            "!if \"${INSTALLMODE}\" == \"perMachine\"",
            "WriteRegStr SHCTX \"${UNINSTKEY}\" \"UninstallString\" \
             \"$\\\"$INSTDIR\\uninstall.exe$\\\" _?=$INSTDIR\"",
            "WriteRegStr SHCTX \"${UNINSTKEY}\" \"QuietUninstallString\" \
             \"$\\\"$INSTDIR\\uninstall.exe$\\\" /S _?=$INSTDIR\"",
            "!else",
            "WriteRegStr SHCTX \"${UNINSTKEY}\" \"UninstallString\" \
             \"$\\\"$INSTDIR\\uninstall.exe$\\\"\"",
            "!endif",
        ]
    );
    // Nothing else in the template writes either value: no QuietUninstallString per user.
    let text = read(TEMPLATE);
    assert_eq!(text.matches("\"QuietUninstallString\"").count(), 1);
    assert_eq!(
        text.matches("WriteRegStr SHCTX \"${UNINSTKEY}\" \"UninstallString\"")
            .count(),
        2
    );
}

#[test]
fn the_reinstall_page_appends_its_own_folder_last_so_nsis_takes_it() {
    // NSIS reads the last " _?=" on the line (Main.c scans back from the end), so the registry's
    // own _?= ahead of /UPDATE, /P and this one changes nothing.
    let lines = span(
        &read(TEMPLATE),
        "Function PageLeaveReinstall",
        "FunctionEnd",
    );
    let read_string = line_with(
        &lines,
        &["ReadRegStr $R1 SHCTX \"${UNINSTKEY}\" \"UninstallString\""],
    );
    assert_run(
        &lines[read_string..],
        &[
            "ReadRegStr $R1 SHCTX \"${UNINSTKEY}\" \"UninstallString\"",
            "${IfThen} $UpdateMode = 1 ${|} StrCpy $R1 \"$R1 /UPDATE\" ${|} ; append /UPDATE",
            "${IfThen} $PassiveMode = 1 ${|} StrCpy $R1 \"$R1 /P\" ${|} ; append /P",
            "StrCpy $R1 \"$R1 _?=$4\" ; append uninstall directory",
            "ExecWait '$R1' $0",
        ],
    );
    // And it never runs the uninstaller with /UPDATE set: update mode skips the uninstall.
    assert_run(
        &lines,
        &["${If} $UpdateMode = 1", "Goto reinst_done", "${EndIf}"],
    );
}

#[test]
fn the_uninstaller_makes_its_plug_in_folder_first_in_un_on_init() {
    let text = read(TEMPLATE);
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "un-admin-section"),
        [
            "!ifmacrodef CLAUSEO_MACHINE_START",
            "!insertmacro CLAUSEO_MACHINE_START",
            "!endif",
        ]
    );
    let on_init = span(&text, "Function un.onInit", "FunctionEnd");
    assert_eq!(on_init[1], "; >>> clauseo: un-admin-section");
    let block_end = line_with(&on_init, &["; <<< clauseo: un-admin-section"]);
    let first_plug_in = on_init
        .iter()
        .position(|line| {
            !line.starts_with(';') && (loads_a_plug_in(line) || line.contains("${GetOptions}"))
        })
        .unwrap();
    assert!(block_end < first_plug_in, "{:?}", on_init[first_plug_in]);
    assert_eq!(on_init[first_plug_in], "!insertmacro SetContext");
    // The start is per machine only (safe_temp.rs pins its body), and its box speaks for the
    // uninstaller there.
    let start = code_of("windows/safe_temp.nsh");
    assert_eq!(
        macro_body(&start, "CLAUSEO_MACHINE_START")[1],
        "!if \"${INSTALLMODE}\" == \"perMachine\""
    );
    assert_run(
        macro_body(&start, "CLAUSEO_SAFE_PLUGINSDIR"),
        &[
            "!ifdef __UNINSTALL__",
            "MessageBox MB_OK|MB_ICONSTOP \"$(uninstallTempFailed)\" /SD IDOK",
            "!else",
        ],
    );
}

#[test]
fn the_self_delete_runs_last_in_the_uninstall_section() {
    let text = read(TEMPLATE);
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "uninstall-self-delete"),
        [
            "!ifmacrodef CLAUSEO_UNINSTALL_SELF_DELETE",
            "!insertmacro CLAUSEO_UNINSTALL_SELF_DELETE",
            "!endif",
        ]
    );
    let section = span(&text, "Section Uninstall", "SectionEnd");
    let block = line_with(&section, &["; >>> clauseo: uninstall-self-delete"]);
    // After everything the section removes, the post-uninstall hook included.
    for done in [
        "Delete \"$INSTDIR\\uninstall.exe\"",
        "RMDir \"$INSTDIR\"",
        "DeleteRegKey HKLM \"${UNINSTKEY}\"",
        "!insertmacro NSIS_HOOK_POSTUNINSTALL",
    ] {
        assert!(line_with(&section, &[done]) < block, "{done}");
    }
    // Only the auto-close follows it.
    let rest: Vec<&str> = section[block..]
        .iter()
        .map(String::as_str)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .collect();
    assert_eq!(
        rest[3..],
        [
            "${If} $PassiveMode = 1",
            "${OrIf} $UpdateMode = 1",
            "SetAutoClose true",
            "${EndIf}",
            "SectionEnd",
        ]
    );
    // The template's own delete of the image names only that file, and its RMDir isn't
    // recursive: failing on the running image, they can't take anything else.
    let deletes: Vec<&String> = section
        .iter()
        .filter(|line| !line.starts_with(';') && line.contains("uninstall.exe"))
        .collect();
    assert_eq!(deletes, ["Delete \"$INSTDIR\\uninstall.exe\""]);
}

#[test]
fn a_path_with_a_cmd_metacharacter_builds_no_command_line() {
    let code = code();
    assert_eq!(
        macro_body(&code, "CLAUSEO_CMD_SAFE")[1..],
        [
            "Push $R0",
            "Push $R1",
            "Push $R2",
            "StrCpy $R2 \"${PATH}\"",
            "StrCpy ${OUT} 1",
            "StrCpy $R0 0",
            "${Do}",
            "StrCpy $R1 $R2 1 $R0",
            "${If} $R1 == \"\"",
            "${ExitDo}",
            "${EndIf}",
            "${If} $R1 == \"%\"",
            "${OrIf} $R1 == \"^\"",
            "${OrIf} $R1 == \"&\"",
            "${OrIf} $R1 == \"|\"",
            "${OrIf} $R1 == \"<\"",
            "${OrIf} $R1 == \">\"",
            "${OrIf} $R1 == \"$\\\"\"",
            "${OrIf} $R1 == \"!\"",
            "StrCpy ${OUT} 0",
            "${ExitDo}",
            "${EndIf}",
            "IntOp $R0 $R0 + 1",
            "${Loop}",
            "Pop $R2",
            "Pop $R1",
            "Pop $R0",
        ]
    );
}

/// The whole cmd line, as NSIS builds it: cmd without `AutoRun` or delayed expansion, PING and
/// both folders by full path, the renamed file (`$1`) deleted, both folders removed only empty.
const COMMAND: &str = "StrCpy $ClauseoSelfDelete '/d /v:off /c \"\"$SYSDIR\\PING.EXE\" -n 4 \
                       127.0.0.1 >nul & del /f /q \"$1\" & rmdir \"$INSTDIR\\installer-temp\" & \
                       rmdir \"$INSTDIR\"\"'";

#[test]
fn the_running_image_is_checked_renamed_and_only_then_given_a_delete() {
    let code = code();
    assert_eq!(
        macro_body(&code, "CLAUSEO_UNINSTALL_SELF_DELETE")[1..],
        [
            "!if \"${INSTALLMODE}\" == \"perMachine\"",
            "Push $0",
            "Push $1",
            "Push $2",
            "StrCpy $ClauseoSelfDelete \"\"",
            // $INSTDIR refused first, before anything is made in it.
            "!insertmacro CLAUSEO_CMD_SAFE $INSTDIR $0",
            "${If} $0 = 1",
            "${AndIf} ${FileExists} \"$INSTDIR\\uninstall.exe\"",
            "ClearErrors",
            "GetTempFileName $1 \"$INSTDIR\"",
            "${IfNot} ${Errors}",
            // The placeholder goes, so Rename (which refuses a target that exists) can use the
            // name's stem.
            "Delete \"$1\"",
            "StrLen $2 $1",
            "${Do}",
            "IntOp $2 $2 - 1",
            "${If} $2 < 0",
            "${ExitDo}",
            "${EndIf}",
            "StrCpy $0 $1 1 $2",
            "${If} $0 == \"\\\"",
            "${ExitDo}",
            "${EndIf}",
            "${Loop}",
            "IntOp $2 $2 + 1",
            "StrCpy $1 $1 \"\" $2",
            "StrCpy $1 $1 -4",
            "StrCpy $1 \"$INSTDIR\\uninstall.$1.old\"",
            // The renamed path refused too, then the rename, and the line only once it worked.
            "!insertmacro CLAUSEO_CMD_SAFE $1 $0",
            "${If} $0 = 1",
            "ClearErrors",
            "Rename \"$INSTDIR\\uninstall.exe\" \"$1\"",
            "${IfNot} ${Errors}",
            COMMAND,
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
            "${EndIf}",
            "Pop $2",
            "Pop $1",
            "Pop $0",
            "${If} ${Silent}",
            "!insertmacro CLAUSEO_START_SELF_DELETE",
            "${EndIf}",
            "!endif",
        ]
    );
    // The comment says why /d is there, and the gap after Delete is said to be admin-only.
    let text = read("windows/uninstall_in_place.nsh");
    assert!(text.contains("; /d: an elevated cmd would otherwise run HKCU's Command Processor"));
    assert!(text.contains("admin-only (Program Files'), so no one else can put anything"));
    // No recursive removal anywhere in the file.
    for line in &code {
        assert!(!line.contains("/s"), "{line}");
        assert!(!line.contains("RMDir /r"), "{line}");
    }
}

#[test]
fn the_command_starts_once_hidden_by_full_path_from_the_section_under_s_else_at_gui_end() {
    let code = code();
    // Starting clears the line, so a second start does nothing.
    assert_eq!(
        macro_body(&code, "CLAUSEO_START_SELF_DELETE")[1..],
        [
            "${If} $ClauseoSelfDelete != \"\"",
            "ExecShell \"open\" \"$SYSDIR\\cmd.exe\" '$ClauseoSelfDelete' SW_HIDE",
            "StrCpy $ClauseoSelfDelete \"\"",
            "${EndIf}",
        ]
    );
    assert_eq!(
        macro_body(&code, "CLAUSEO_UNINSTALL_GUI_END")[1..],
        ["!insertmacro CLAUSEO_START_SELF_DELETE"]
    );
    // The two places it starts: the section under /S only (NSIS calls no un.onGUIEnd without a
    // window), and un.onGUIEnd, which interactive and /P runs (both have a window) reach. /P isn't
    // silent, so it takes un.onGUIEnd's start and not the section's.
    let starts = code
        .iter()
        .filter(|line| *line == "!insertmacro CLAUSEO_START_SELF_DELETE")
        .count();
    assert_eq!(starts, 2);
    let section = macro_body(&code, "CLAUSEO_UNINSTALL_SELF_DELETE");
    let start = line_with(section, &["!insertmacro CLAUSEO_START_SELF_DELETE"]);
    assert_eq!(section[start - 1], "${If} ${Silent}");
    assert!(!code.iter().any(|line| line.contains("PassiveMode")));
    let text = read(TEMPLATE);
    let (blocks, _) = split_blocks(&text);
    assert_eq!(
        block_code(&blocks, "un-gui-end"),
        [
            "Function un.onGUIEnd",
            "!ifmacrodef CLAUSEO_UNINSTALL_GUI_END",
            "!insertmacro CLAUSEO_UNINSTALL_GUI_END",
            "!endif",
            "FunctionEnd",
        ]
    );
    // The only un.onGUIEnd, right after the uninstall section.
    assert_eq!(text.matches("Function un.onGUIEnd").count(), 1);
    let after = span(&text, "Section Uninstall", "; >>> clauseo: un-gui-end");
    assert_eq!(after[after.len() - 2], "SectionEnd");
    // Declared once, and included where the other installer macros are.
    line_with(&code, &["Var ClauseoSelfDelete"]);
    let copy = read("windows/admin_copy.nsh");
    assert!(copy.contains("!include \"${__FILEDIR__}\\uninstall_in_place.nsh\""));
}
