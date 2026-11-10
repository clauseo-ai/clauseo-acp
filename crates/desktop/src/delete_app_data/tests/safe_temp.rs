//! The elevated installers' own temporary folder (`windows/safe_temp.nsh`): made fresh under
//! Program Files, owned by Administrators and locked to Administrators and SYSTEM, then set as
//! `$PLUGINSDIR` and `$TEMP`, first in every elevated path and before any plug-in call: in the
//! per-user installer's elevated branch and at the start of the per-machine installer's
//! `.onInit`. Each per-machine run then removes earlier runs' folders, run here on a fake folder
//! listing. The Windows VM checks the folder and its DACL for real.

use super::admin_copy::{admin_code, macro_body};
use super::nsis_run::{Machine, defines, parse_all, words};
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
            // Last, with System loaded from this run's folder: earlier runs' folders.
            "!insertmacro CLAUSEO_REMOVE_OLD_PLUGINSDIRS",
            "!endif",
        ]
    );
    // And the folder's own macro loads nothing, nor does the removal.
    for start in ["CLAUSEO_SAFE_PLUGINSDIR", "CLAUSEO_REMOVE_OLD_PLUGINSDIRS"] {
        for (name, body) in reached(&code, start) {
            for line in body {
                assert!(!loads_a_plug_in(line), "{name}: {line}");
            }
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
    // Nothing the branch reaches loads a plug-in, before the folder or after it. It removes no
    // earlier folder either: per user stays as it was (its folder holds no plug-in, so NSIS's exit
    // removes it).
    for (name, body) in reached(&code, "CLAUSEO_ELEVATED_BRANCH") {
        assert_ne!(name, "CLAUSEO_REMOVE_OLD_PLUGINSDIRS");
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
        &["!define CLAUSEO_TEMP_PARENT \"$PROGRAMFILES64\\Clauseo\""],
    );
    line_with(
        &code,
        &["!define CLAUSEO_OLD_TEMP_PARENT \"$PROGRAMFILES64\\Clauseo\\installer-temp\""],
    );
    line_with(
        &code,
        &["!define CLAUSEO_SID_ADMINISTRATORS \"*S-1-5-32-544\""],
    );
    line_with(&code, &["!define CLAUSEO_SID_SYSTEM \"*S-1-5-18\""]);
    line_with(&code, &["!define CLAUSEO_EXIT_TEMP_FAILED 25"]);
    // Every step, in order: earlier builds' parent removed only when empty (never /r: an older
    // install running meanwhile keeps its folder), a name GetTempFileName made free directly in
    // %ProgramFiles%\Clauseo (no parent of its own to stay behind), a folder in its place, icacls'
    // owner and protected DACL (each exit code 0, a start that failed counted as a failure), then
    // $PLUGINSDIR and $TEMP. Any failure removes what it made, never recursively, and quits.
    assert_eq!(
        macro_body(&code, "CLAUSEO_SAFE_PLUGINSDIR")[1..],
        [
            "Push $0",
            "Push $1",
            "RMDir \"${CLAUSEO_OLD_TEMP_PARENT}\"",
            "StrCpy $1 1",
            "ClearErrors",
            "CreateDirectory \"${CLAUSEO_TEMP_PARENT}\"",
            "GetTempFileName $0 \"${CLAUSEO_TEMP_PARENT}\"",
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
            "MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST \"$(uninstallTempFailed)\" /SD IDOK",
            "!else",
            "MessageBox MB_OK|MB_ICONSTOP|MB_SETFOREGROUND|MB_TOPMOST \"$(installTempFailed)\" /SD IDOK",
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

/// The lines of macro `name` with `args` put in for its parameters (NSIS drops an argument's
/// quotes), each macro it inserts inlined the same way.
fn inlined(code: &[String], name: &str, args: &[&str]) -> Vec<String> {
    let body = macro_body(code, name);
    let params = words(&body[0]);
    assert_eq!(params.len() - 2, args.len(), "{}", body[0]);
    let mut out = Vec::new();
    for line in &body[1..] {
        let mut line = line.clone();
        for (param, arg) in params[2..].iter().zip(args) {
            line = line.replace(&format!("${{{param}}}"), arg);
        }
        let parts = words(&line);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        if let ["!insertmacro", inner, rest @ ..] = parts.as_slice() {
            out.extend(inlined(code, inner, rest));
        } else {
            out.push(line);
        }
    }
    out
}

/// What `CLAUSEO_NSIS_TEMP_NAME` says of `name`, run as written.
fn nsis_temp_name(name: &str) -> bool {
    let code = admin_code();
    let nodes = parse_all(&inlined(&code, "CLAUSEO_NSIS_TEMP_NAME", &["$1", "$2"]));
    let mut machine = Machine::new(defines(&code), None);
    machine.set_var("$1", name);
    assert!(!machine.run(&nodes), "{name}");
    assert!(machine.stack.is_empty(), "{name}");
    machine.vars["$2"] == "1"
}

#[test]
fn a_folder_counts_as_an_nsis_one_only_by_the_name_nsis_gives_it() {
    // "ns", a letter, 1-4 hex digits, ".tmp" (my_GetTempFileName, util.c:473-485, then Win32's
    // GetTempFileName), any case: the three the smoke found, and the edges.
    for name in [
        "nsoA5E5.tmp",
        "nsu9CAD.tmp",
        "nsz8DF8.tmp",
        "nsa0.tmp",
        "nsqffff.tmp",
        "NSB12.TMP",
    ] {
        assert!(nsis_temp_name(name), "{name}");
    }
    for name in [
        "",
        "Sandbox",
        "uninstall.exe",
        "ns.tmp",
        "nsa.tmp",
        "ns1A5E5.tmp",
        "nsaG5E5.tmp",
        "nsaA5E5F.tmp",
        "nsa5 5.tmp",
        "xsa5E5.tmp",
        "nsa5E5.tmpx",
        "nsa5E5.tmp.old",
        "nsa5E5.dll",
    ] {
        assert!(!nsis_temp_name(name), "{name}");
    }
}

#[test]
fn earlier_runs_folders_go_only_when_named_by_nsis_and_their_system_dll_could_go() {
    let code = admin_code();
    let nodes = parse_all(&inlined(&code, "CLAUSEO_REMOVE_OLD_PLUGINSDIRS", &[]));
    let mut machine = Machine::new(defines(&code), None);
    let parent = "$PROGRAMFILES64\\Clauseo";
    let at = |name: &str| format!("{parent}\\{name}");
    // This run's folder, and an install folder that happens to have an NSIS name.
    machine.set_var("$PLUGINSDIR", &at("nsb0001.tmp"));
    machine.set_var("$INSTDIR", &at("nsd0002.tmp"));
    // What FindFirst finds: two earlier runs' folders, this run's, one whose System.dll a run
    // still has loaded, one with no System.dll (a run that hasn't loaded it yet), the install
    // folder, and a name only an 8.3 match of ns*.tmp returns.
    machine.found = [
        "nsoA5E5.tmp",
        "nsb0001.tmp",
        "nsz8DF8.tmp",
        "nsc12.tmp",
        "nsd0002.tmp",
        "nsfoo.tmpl",
        "nsu9CAD.tmp",
    ]
    .map(str::to_owned)
    .to_vec();
    machine.files = [
        "nsoA5E5.tmp",
        "nsb0001.tmp",
        "nsz8DF8.tmp",
        "nsd0002.tmp",
        "nsfoo.tmpl",
        "nsu9CAD.tmp",
    ]
    .iter()
    .map(|name| at(&format!("{name}\\System.dll")))
    .collect();
    machine.locked = vec![at("nsz8DF8.tmp\\System.dll")];
    machine.error = true;
    assert!(!machine.run(&nodes));
    assert_eq!(
        machine.said,
        [
            format!("FindFirst {parent}\\ns*.tmp"),
            format!("Delete {}", at("nsoA5E5.tmp\\System.dll")),
            format!("RMDir /r {}", at("nsoA5E5.tmp")),
            format!("Delete {}", at("nsz8DF8.tmp\\System.dll")),
            format!("Delete {}", at("nsu9CAD.tmp\\System.dll")),
            format!("RMDir /r {}", at("nsu9CAD.tmp")),
        ]
    );
    // The error flag clear, as the start leaves it, and the registers back.
    assert!(!machine.error);
    assert!(machine.stack.is_empty());
    // The only recursive delete in the elevated code is that one, on a path in
    // CLAUSEO_TEMP_PARENT.
    let recursive: Vec<&String> = code
        .iter()
        .filter(|line| line.contains("RMDir /r"))
        .collect();
    assert_eq!(recursive, ["RMDir /r \"$3\""]);
    let body = macro_body(&code, "CLAUSEO_REMOVE_OLD_PLUGINSDIRS");
    line_with(body, &["StrCpy $3 \"${CLAUSEO_TEMP_PARENT}\\$1\""]);
    line_with(body, &["RMDir /r \"$3\""]);
}

/// What `CLAUSEO_OWN_PLUGINSDIR` says of `plugins_dir`, run as written.
fn own_plugins_dir(plugins_dir: &str) -> bool {
    let code = admin_code();
    let nodes = parse_all(&inlined(&code, "CLAUSEO_OWN_PLUGINSDIR", &["$0"]));
    let mut machine = Machine::new(defines(&code), None);
    machine.set_var("$PLUGINSDIR", plugins_dir);
    assert!(!machine.run(&nodes), "{plugins_dir}");
    assert!(machine.stack.is_empty(), "{plugins_dir}");
    machine.vars["$0"] == "1"
}

#[test]
fn the_uninstaller_removes_only_a_plug_in_folder_directly_in_program_files_clauseo() {
    // The folder CLAUSEO_SAFE_PLUGINSDIR makes, which the uninstaller's delayed cmd line removes
    // with rmdir /s (uninstall_in_place.rs); nothing else, $INSTDIR included.
    assert!(own_plugins_dir("$PROGRAMFILES64\\Clauseo\\nsoA5E5.tmp"));
    for path in [
        "",
        "$PROGRAMFILES64\\Clauseo",
        "$PROGRAMFILES64\\Clauseo\\",
        "$PROGRAMFILES64\\Clauseo\\Sandbox",
        "$PROGRAMFILES64\\Clauseo\\nsoA5E5.tmp\\",
        "$PROGRAMFILES64\\Clauseo\\nsoA5E5.tmp\\nsu9CAD.tmp",
        "$PROGRAMFILES64\\Clauseo2\\nsoA5E5.tmp",
        "C:\\Users\\lawyer\\AppData\\Local\\Temp\\nsoA5E5.tmp",
    ] {
        assert!(!own_plugins_dir(path), "{path}");
    }
}
