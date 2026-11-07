//! What the sources must keep: the uninstall hook closes the app, runs S15's removal (with the
//! setup exe's own arguments and exit 0, waiting at most two minutes), then runs
//! `--delete-app-data` with the box cleared before the template's `RmDir /r`, with the exit codes,
//! folder names and words it names; and nothing a window can reach (commands, the bridge,
//! capabilities, permissions) names the deletion.

use std::path::{Path, PathBuf};

use crate::delete_app_data::walk::{SANDBOX_DIR, SET_ASIDE_PATH};
use crate::delete_app_data::{
    FLAG, KEPT_AND_FAILED, KEPT_SANDBOX, KEPT_SET_ASIDE, SANDBOX_REMOVED,
};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

pub(super) fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

/// `hooks.nsh`'s lines, comments and blank lines left out, trimmed.
pub(super) fn hook_code() -> Vec<String> {
    read("windows/hooks.nsh")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect()
}

/// The index of the first code line holding all of `parts`.
pub(super) fn line_with(code: &[String], parts: &[&str]) -> usize {
    code.iter()
        .position(|line| parts.iter().all(|part| line.contains(part)))
        .unwrap_or_else(|| panic!("no line in hooks.nsh holds {parts:?}"))
}

/// The pre-uninstall macro's code lines.
pub(super) fn preuninstall(code: &[String]) -> &[String] {
    let start = line_with(code, &["!macro NSIS_HOOK_PREUNINSTALL"]);
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

#[test]
fn the_uninstall_hook_checks_the_app_then_removes_s15s_set_up_then_runs_the_deletion() {
    let code = hook_code();
    let body = preuninstall(&code);
    let app_check = line_with(body, &["!insertmacro CheckIfAppIsRunning"]);
    let remove = line_with(body, &["clauseo-sandbox-setup.exe", "${CLAUSEO_REMOVE}"]);
    let cleared = line_with(body, &["StrCpy $DeleteAppDataCheckboxState 0"]);
    let exe = "\"$INSTDIR\\${MAINBINARYNAME}.exe\"";
    let runs = line_with(body, &["ExecWait", &format!("{exe} {FLAG}' $R7")]);
    let runs_removed = line_with(
        body,
        &["ExecWait", &format!("{exe} {FLAG} {SANDBOX_REMOVED}' $R7")],
    );
    assert!(
        app_check < remove,
        "the app is closed before S15's --remove"
    );
    assert!(
        remove < cleared,
        "S15's --remove runs before anything is deleted"
    );
    assert!(
        cleared < runs.min(runs_removed),
        "the box is cleared before the app runs"
    );
    // --sandbox-removed only after the removal exited 0: $R5 starts at 0 and becomes 1 only
    // right under the exit code's test, and only $R5 = 1 picks the call that passes it.
    let unremoved = line_with(body, &["StrCpy $R5 0"]);
    let exited_zero = line_with(body, &["${If} $R8 = 0"]);
    let removed = line_with(body, &["StrCpy $R5 1"]);
    assert!(unremoved < remove, "{body:?}");
    assert_eq!(exited_zero + 1, removed, "{body:?}");
    let sets = body.iter().filter(|line| line.contains("StrCpy $R5 1"));
    assert_eq!(sets.count(), 1, "{body:?}");
    assert_eq!(
        line_with(body, &["${If} $R5 = 1"]) + 1,
        runs_removed,
        "{body:?}"
    );
    // fMask: SEE_MASK_NOCLOSEPROCESS (0x40) for the exit code, and SEE_MASK_FLAG_NO_UI (0x400)
    // so a failed call (a declined prompt, 1223; a blocked program, 1260) shows no error box of
    // Windows' own. The flag does not suppress the consent prompt. A failed call leaves $R5 at 0.
    let info = line_with(body, &["System::Call '*(&l4, i "]);
    let hex = body[info]
        .split_once("(&l4, i ")
        .and_then(|(_, rest)| rest.split_once(','))
        .and_then(|(mask, _)| mask.strip_prefix("0x"))
        .unwrap_or_else(|| panic!("no hex fMask in {}", body[info]));
    let mask = u32::from_str_radix(hex, 16).unwrap();
    assert_eq!(mask, 0x0000_0400 | 0x0000_0040, "{}", body[info]);
    let call = line_with(body, &["shell32::ShellExecuteExW(p R6) i .R7"]);
    assert_eq!(info + 1, call, "{body:?}");
    assert!(call < exited_zero, "{body:?}");
    // The box tested once and cleared once; nothing in the file deletes a tree itself.
    let box_lines = code
        .iter()
        .filter(|line| line.contains("$DeleteAppDataCheckboxState"));
    assert_eq!(box_lines.count(), 2, "{body:?}");
    for line in &code {
        let lower = line.to_lowercase();
        assert!(!lower.contains("rmdir"), "{line}");
        assert!(!lower.contains("rd /s"), "{line}");
    }
}

#[test]
fn the_hooks_removal_passes_the_exes_own_arguments_and_reads_its_0_within_two_minutes() {
    use clauseo_sandbox::windows::setup::args::{Mode, PER_MACHINE, parse};
    use clauseo_sandbox::windows::setup::{REMOVE_LOCK_WAIT_MS, SetupExit};

    let code = hook_code();
    let body = preuninstall(&code);
    // A finished removal is exit 0 (`SetupExit::Removed`), and only that sets $R5.
    let removed = SetupExit::Removed.as_code();
    assert_eq!(removed, 0);
    let exited = line_with(body, &[&format!("${{If}} $R8 = {removed}")]);
    assert_eq!(body[exited + 1], "StrCpy $R5 1", "{body:?}");
    // The arguments: exactly `--remove`, and `--per-machine` after it in the per-machine
    // installer only, chosen when the installer is built (stage F), never at run time. The
    // per-machine uninstaller is elevated already, so it opens the tool ("open"); the per-user
    // one asks for elevation ("runas"). Only those two install modes build.
    let mode = line_with(body, &["!if \"${INSTALLMODE}\" == \"perMachine\""]);
    let per_machine = format!("!define CLAUSEO_REMOVE \"--remove {PER_MACHINE}\"");
    let expected = [
        "!ifndef CLAUSEO_REMOVE",
        "!if \"${INSTALLMODE}\" == \"perMachine\"",
        per_machine.as_str(),
        "!else",
        "!define CLAUSEO_REMOVE \"--remove\"",
        "!endif",
        "!endif",
        // Each defined once, however often the macro is inserted.
        "!ifndef CLAUSEO_REMOVE_VERB",
        "!if \"${INSTALLMODE}\" == \"perMachine\"",
        "!define CLAUSEO_REMOVE_VERB \"open\"",
        "!else",
        "!define CLAUSEO_REMOVE_VERB \"runas\"",
        "!endif",
        "!endif",
    ];
    assert_eq!(body[mode - 1..mode + 13], expected, "{body:?}");
    let ifndef = line_with(body, &["!ifndef INSTALLMODE"]);
    assert!(body[ifndef + 1].starts_with("!error "), "{body:?}");
    let other = line_with(body, &["!if \"${INSTALLMODE}\" != \"perMachine\""]);
    assert_eq!(
        body[other + 1],
        "!if \"${INSTALLMODE}\" != \"currentUser\"",
        "{body:?}"
    );
    assert!(body[other + 2].starts_with("!error "), "{body:?}");
    let call = line_with(
        body,
        &[concat!(
            "w \"${CLAUSEO_REMOVE_VERB}\", w \"$INSTDIR\\clauseo-sandbox-setup.exe\", ",
            "w \"${CLAUSEO_REMOVE}\", p 0,"
        )],
    );
    assert!(
        ifndef < other && other < mode && mode < call && call < exited,
        "{body:?}"
    );
    // The exe takes both forms as they are written, and nothing else from the hook.
    let words = |line: &str| -> Vec<std::ffi::OsString> {
        line.split(' ').map(std::ffi::OsString::from).collect()
    };
    let parsed = |line: &str| parse(words(line));
    assert_eq!(parsed("--remove"), Ok(Mode::Remove { per_machine: false }));
    assert_eq!(
        parsed(&format!("--remove {PER_MACHINE}")),
        Ok(Mode::Remove { per_machine: true })
    );
    // At most two minutes, then on as after a failure: $R8 is seeded 1 (a failure) before the
    // wait, and only WAIT_OBJECT_0 (0) reads the exit code; a timeout (WAIT_TIMEOUT, 258) or
    // WAIT_FAILED never does. The exe's own lock wait fits inside it.
    let wait = line_with(body, &["kernel32::WaitForSingleObject(p R7, i ", ") i .R3"]);
    let bound: u32 = body[wait]
        .split_once("(p R7, i ")
        .and_then(|(_, rest)| rest.split_once(')'))
        .and_then(|(ms, _)| ms.parse().ok())
        .unwrap_or_else(|| panic!("no wait in {}", body[wait]));
    assert_eq!(bound, 120_000, "{}", body[wait]);
    assert_eq!(body[wait - 1], "StrCpy $R8 1", "{body:?}");
    assert_eq!(body[wait + 1], "${If} $R3 = 258", "{body:?}");
    // The timeout's line is English.nsh's, in the details list only.
    assert_eq!(
        body[wait + 2],
        "DetailPrint \"$(removeStillRunning)\"",
        "{body:?}"
    );
    let english = read("windows/nsis/English.nsh");
    let still = "LangString removeStillRunning ${LANG_ENGLISH} \"Clauseo is still removing the \
                 Safe workspace. It finishes on its own.\"";
    assert!(english.lines().any(|line| line == still), "{still}");
    // The exit code, read only in the WAIT_OBJECT_0 branch, into a value seeded 1; a failed
    // GetExitCodeProcess (0 in $R2) puts the failure back. Nothing else writes $R8 to 0.
    let ended = line_with(body, &["${ElseIf} $R3 = 0"]);
    assert_eq!(ended, wait + 3, "{body:?}");
    assert_eq!(
        body[ended + 1],
        "System::Call 'kernel32::GetExitCodeProcess(p R7, *i 1 R8) i .R2'",
        "{body:?}"
    );
    assert_eq!(
        body[ended + 2..ended + 6],
        ["${If} $R2 = 0", "StrCpy $R8 1", "${EndIf}", "${EndIf}"],
        "{body:?}"
    );
    let reads = body
        .iter()
        .filter(|line| line.contains("GetExitCodeProcess"));
    assert_eq!(reads.count(), 1, "{body:?}");
    let r8_writes: Vec<&String> = body
        .iter()
        .filter(|line| line.contains("StrCpy $R8 ") || line.contains("R8)"))
        .collect();
    assert!(
        r8_writes.iter().all(|line| !line.contains("StrCpy $R8 0")),
        "{r8_writes:?}"
    );
    let close = line_with(body, &["kernel32::CloseHandle(p R7)"]);
    assert_eq!(close, ended + 6, "{body:?}");
    assert!(close < exited, "{body:?}");
    assert!(REMOVE_LOCK_WAIT_MS < bound);
    // The mode is never read at run time, and the tool is never killed.
    for line in &code {
        assert!(!line.contains("MultiUser"), "{line}");
        assert!(!line.to_lowercase().contains("terminateprocess"), "{line}");
        assert!(!line.contains("WaitForSingleObject(p R7, i -1)"), "{line}");
    }
}

#[test]
fn a_silent_per_user_uninstall_skips_the_elevated_removal_and_keeps_the_sandbox_folder() {
    let code = hook_code();
    let body = preuninstall(&code);
    // In the per-user installer only (chosen when it is built), /S clears $R3 before the
    // removal's test, says so in the details list and shows no box; $R3 0 skips the whole
    // ShellExecuteExW block, so $R5 stays 0 and --sandbox-removed is never passed, as after a
    // declined prompt.
    let run = line_with(body, &["StrCpy $R3 1"]);
    let expected = [
        "StrCpy $R3 1",
        "!if \"${INSTALLMODE}\" == \"currentUser\"",
        "${If} ${Silent}",
        "${AndIf} ${FileExists} \"$INSTDIR\\clauseo-sandbox-setup.exe\"",
        "StrCpy $R3 0",
        "DetailPrint \"$(removeSkippedSilent)\"",
        "${EndIf}",
        "!endif",
        "${If} $R3 = 1",
        "${AndIf} ${FileExists} \"$INSTDIR\\clauseo-sandbox-setup.exe\"",
    ];
    assert_eq!(body[run..run + 10], expected, "{body:?}");
    let unremoved = line_with(body, &["StrCpy $R5 0"]);
    let call = line_with(body, &["shell32::ShellExecuteExW(p R6) i .R7"]);
    assert!(unremoved < run && run + 10 < call, "{body:?}");
    // The skip shows no box: nothing between it and the removal's test is a MessageBox.
    assert!(
        body[run..run + 10]
            .iter()
            .all(|line| !line.contains("MessageBox")),
        "{body:?}"
    );
    let english = read("windows/nsis/English.nsh");
    // Stage F's words (#619), provisional: they say how to remove the Safe workspace.
    let skipped = "LangString removeSkippedSilent ${LANG_ENGLISH} \"Clauseo didn't remove the \
                   Safe workspace, because this uninstall is silent. To remove it, reinstall Clauseo \
                   and uninstall it without /S.\"";
    assert!(english.lines().any(|line| line == skipped), "{skipped}");
}

#[test]
fn the_apps_language_goes_with_the_data_and_stays_when_it_is_kept() {
    let code = hook_code();
    let body = preuninstall(&code);
    // The app writes HKCU\Software\Clauseo\UiLanguage; ${MANUKEY} is "Software\${MANUFACTURER}"
    // (installer.nsi:68), and MANUFACTURER is tauri.conf.json's publisher.
    let conf: serde_json::Value = serde_json::from_str(&read("tauri.conf.json")).unwrap();
    assert_eq!(conf["bundle"]["publisher"], "Clauseo");
    let deleted = line_with(body, &[r#"DeleteRegValue HKCU "${MANUKEY}" "UiLanguage""#]);
    let key = line_with(body, &[r#"DeleteRegKey /ifempty HKCU "${MANUKEY}""#]);
    assert_eq!(deleted + 1, key, "the value goes right before its key");
    // Only in the box's branch: after the box is cleared there, and nowhere else in the hooks.
    let branch = line_with(body, &["${If} $DeleteAppDataCheckboxState = 1"]);
    let cleared = line_with(body, &["StrCpy $DeleteAppDataCheckboxState 0"]);
    assert!(branch < cleared && cleared < deleted, "{body:?}");
    let deletes = code.iter().filter(|line| line.starts_with("Delete"));
    assert_eq!(
        deletes.filter(|line| line.contains("UiLanguage")).count(),
        1
    );
}

#[test]
fn the_hooks_messages_name_the_kept_folder_through_localappdata() {
    let code = hook_code();
    let body = preuninstall(&code);
    // The folder from NSIS's $LOCALAPPDATA in the user's context, never from the environment.
    let current = line_with(body, &["SetShellVarContext current"]);
    let named = line_with(body, &["StrCpy $R9 \"$LOCALAPPDATA\\${BUNDLEID}\""]);
    let put_back = line_with(body, &["SetShellVarContext all"]);
    assert!(current < named && named < put_back, "{body:?}");
    assert!(!code.iter().any(|line| line.contains("ReadEnvStr")));
    // The kept folders, named as the app names them.
    line_with(
        body,
        &[&format!(
            "StrCpy $R8 \"$R9\\{}\"",
            SET_ASIDE_PATH.join("\\")
        )],
    );
    line_with(body, &[&format!("StrCpy $R8 \"$R9\\{SANDBOX_DIR}\"")]);
    // After --sandbox-removed, a keep with a failure is the set-aside's.
    let kept_and_failed = line_with(body, &["${ElseIf} $R7 = ${CLAUSEO_KEPT_AND_FAILED}"]);
    assert_eq!(body[kept_and_failed + 1], "${AndIf} $R5 = 1");
    assert_eq!(body[kept_and_failed + 2], "${AndIf} $R2 = 1");
    assert!(
        body[kept_and_failed + 3].ends_with("\\set-aside\""),
        "{body:?}"
    );
    // One box: Failed only when no folder stays; Kept when one stays and all else went (2, 3);
    // KeptFailed, naming both, otherwise (4, or a failure with `sandbox` still there).
    let failed = line_with(body, &["$(deleteAppDataFailed)"]);
    let kept = line_with(body, &["$(deleteAppDataKept)\""]);
    let both = line_with(body, &["$(deleteAppDataKeptFailed)"]);
    assert_eq!(body[failed - 1], "${ElseIf} $R8 == \"\"");
    assert_eq!(body[kept - 2], "${ElseIf} $R7 = ${CLAUSEO_KEPT_SANDBOX}");
    assert_eq!(body[kept - 1], "${OrIf} $R7 = ${CLAUSEO_KEPT_SET_ASIDE}");
    assert_eq!(body[both - 1], "${Else}");
    assert_eq!(body[both + 1], "${EndIf}");
    // The data folder's boxes: those after step 2 starts (the one before is removeExeMissing's).
    let deletion = line_with(body, &["${If} $DeleteAppDataCheckboxState = 1"]);
    let boxes = body[deletion..]
        .iter()
        .filter(|line| line.starts_with("MessageBox"));
    assert_eq!(boxes.count(), 4, "{body:?}");
}

#[test]
fn the_hook_names_a_kept_folder_only_when_it_is_a_real_folder() {
    let code = hook_code();
    // The check: the path's own attributes, a folder and no reparse point, never through a link
    // (${FileExists} follows one); INVALID_FILE_ATTRIBUTES is no folder.
    let check = line_with(&code, &["!macro CLAUSEO_REAL_FOLDER PATH"]);
    // FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY masked, the directory bit alone left.
    let (reparse, directory) = (0x400, 0x10);
    let expected = [
        "StrCpy $R4 0".to_owned(),
        "System::Call 'kernel32::GetFileAttributesW(w \"${PATH}\") i .R3'".to_owned(),
        "${If} $R3 <> -1".to_owned(),
        format!("IntOp $R3 $R3 & {}", reparse | directory),
        format!("${{If}} $R3 = {directory}"),
        "StrCpy $R4 1".to_owned(),
    ];
    assert_eq!(
        code[check + 1..=check + expected.len()],
        expected,
        "{code:?}"
    );
    assert!(check < line_with(&code, &["!macro NSIS_HOOK_PREUNINSTALL"]));
    // Both folders checked before either is named: the set-aside into $R2, `sandbox` into $R4.
    let body = preuninstall(&code);
    let set_aside = SET_ASIDE_PATH.join("\\");
    let checked_set_aside = line_with(
        body,
        &[&format!(
            "!insertmacro CLAUSEO_REAL_FOLDER \"$R9\\{set_aside}\""
        )],
    );
    assert_eq!(body[checked_set_aside + 1], "StrCpy $R2 $R4");
    assert_eq!(
        body[checked_set_aside + 2],
        format!("!insertmacro CLAUSEO_REAL_FOLDER \"$R9\\{SANDBOX_DIR}\"")
    );
    // The set-aside's check reads through `sandbox`: it counts only when `sandbox` is real
    // too, the two joined with AND after both checks and before any name.
    let anded = checked_set_aside + 3;
    assert_eq!(body[anded], "IntOp $R2 $R2 & $R4", "{body:?}");
    // Each name only under its folder's check.
    let names = body
        .iter()
        .enumerate()
        .filter(|(_, line)| line.starts_with("StrCpy $R8 \"$R9"));
    let mut seen = 0;
    for (at, line) in names {
        assert!(anded < at, "{body:?}");
        let guard = if line.ends_with("\\set-aside\"") {
            "${AndIf} $R2 = 1"
        } else {
            "${ElseIf} $R4 = 1"
        };
        assert_eq!(body[at - 1], guard, "{line}");
        seen += 1;
    }
    assert_eq!(seen, 3, "{body:?}");
    assert!(
        !code
            .iter()
            .any(|line| line.contains("FileExists") && line.contains("$R9")),
        "{code:?}"
    );
}

#[test]
fn the_hook_shows_no_box_when_the_data_folder_is_gone_and_no_path_through_a_link() {
    let code = hook_code();
    let body = preuninstall(&code);
    // The data folder's own attributes, with GetLastError: gone only for 2 or 3.
    let attributes = line_with(
        body,
        &["System::Call 'kernel32::GetFileAttributesW(w \"$R9\") i .R3 ?e'"],
    );
    let expected = [
        "Pop $R1",
        "StrCpy $R6 1",
        "${If} $R3 = -1",
        "StrCpy $R6 2",
        "${If} $R1 = 2",
        "${OrIf} $R1 = 3",
        "StrCpy $R6 0",
        "${EndIf}",
        "${Else}",
        "IntOp $R3 $R3 & 1024",
        "${If} $R3 <> 0",
        "StrCpy $R6 2",
    ];
    assert_eq!(
        body[attributes + 1..=attributes + expected.len()],
        expected,
        "{body:?}"
    );
    // Every box sits under `$R6 <> 0` (no box when gone); a present folder gets a box, the
    // unnamed one first for a link or an unreadable one.
    let silent = line_with(body, &["${IfNot} ${Silent}"]);
    assert!(attributes < silent, "{body:?}");
    assert_eq!(body[silent + 1], "${AndIf} $R7 <> ${CLAUSEO_DELETED}");
    assert_eq!(body[silent + 2], "${AndIf} $R6 <> 0");
    assert_eq!(body[silent + 3], "${If} $R6 = 2");
    assert_eq!(
        body[silent + 4],
        "MessageBox MB_OK|MB_ICONEXCLAMATION \"$(deleteAppDataFailedUnnamed)\""
    );
    assert_eq!(body[silent + 5], "${ElseIf} $R8 == \"\"");
    let deletion = line_with(body, &["${If} $DeleteAppDataCheckboxState = 1"]);
    let boxes = (deletion..body.len()).filter(|&at| body[at].starts_with("MessageBox"));
    for at in boxes {
        assert!(silent + 3 < at, "{body:?}");
    }
    // The unnamed line names no path, only the folder's name.
    let english = read("windows/nsis/English.nsh");
    let unnamed = english
        .lines()
        .find_map(|line| {
            line.strip_prefix("LangString deleteAppDataFailedUnnamed ${LANG_ENGLISH} ")
        })
        .unwrap();
    assert!(!unnamed.contains('$'), "{unnamed}");
    assert!(unnamed.contains(clauseo_settings::IDENTIFIER), "{unnamed}");
}

#[test]
fn the_hooks_exit_codes_and_words_are_the_apps() {
    let code = hook_code();
    for (name, value) in [
        ("CLAUSEO_DELETED", super::DELETED),
        ("CLAUSEO_KEPT_SANDBOX", KEPT_SANDBOX),
        ("CLAUSEO_KEPT_SET_ASIDE", KEPT_SET_ASIDE),
        ("CLAUSEO_KEPT_AND_FAILED", KEPT_AND_FAILED),
    ] {
        line_with(&code, &[&format!("!define {name} {value}")]);
    }
    let english = read("windows/nsis/English.nsh");
    for (key, register) in [("deleteAppDataFailed", "$R9"), ("deleteAppDataKept", "$R8")] {
        line_with(
            &code,
            &["MessageBox MB_OK|MB_ICONEXCLAMATION", &format!("$({key})")],
        );
        // The path stands alone after a blank line.
        let line = english
            .lines()
            .find_map(|line| line.strip_prefix(&format!("LangString {key} ${{LANG_ENGLISH}} ")))
            .unwrap_or_else(|| panic!("{key}"));
        assert!(line.ends_with(&format!(".$\\n$\\n{register}\"")), "{line}");
    }
    // The combined box names the kept folder, then the rest.
    line_with(
        &code,
        &[
            "MessageBox MB_OK|MB_ICONEXCLAMATION",
            "$(deleteAppDataKeptFailed)",
        ],
    );
    let both = english
        .lines()
        .find_map(|line| line.strip_prefix("LangString deleteAppDataKeptFailed ${LANG_ENGLISH} "))
        .unwrap();
    assert!(
        both.contains("kept:$\\n$R8$\\n") && both.ends_with("data:$\\n$R9\""),
        "{both}"
    );
    // The messages never show under /S.
    line_with(&code, &["${IfNot} ${Silent}"]);
}

/// Every file under `dir`, recursively.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_owned()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                found.push(path);
            }
        }
    }
    found
}

/// Whether `line` names the module, its function or the stage's method (`delete_app_data` as a
/// whole word: a path, a call, a `use` with or without `as`), or the flag.
fn names_it(line: &str) -> bool {
    let word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.contains(FLAG)
        || line.match_indices("delete_app_data").any(|(at, name)| {
            let before = line[..at].chars().next_back();
            let after = line[at + name.len()..].chars().next();
            !before.is_some_and(word) && !after.is_some_and(word)
        })
}

#[test]
fn every_way_of_naming_the_deletion_is_seen() {
    for named in [
        "AppStages.delete_app_data(&args);",
        "use crate::{delete_app_data as d};",
        "use crate::{bridge, delete_app_data};",
        "crate::delete_app_data::run(args, &mut err)",
        "mod delete_app_data;",
        "\"--delete-app-data\"",
    ] {
        assert!(names_it(named), "{named}");
    }
    for unnamed in [
        "fn the_delete_app_data_box_says_clauseos_words() {",
        "deleteAppData",
    ] {
        assert!(!names_it(unnamed), "{unnamed}");
    }
}

#[test]
fn nothing_a_window_can_reach_names_the_deletion() {
    let dir = manifest_dir();
    // `main` reaches it through `launch` (jailed.rs) alone; lib.rs only declares the module.
    let allowed = |relative: &str| {
        relative == "src/delete_app_data.rs"
            || relative.starts_with("src/delete_app_data/")
            || relative == "src/jailed.rs"
            || relative == "src/jailed/tests.rs"
    };
    let mut looked = 0;
    for folder in ["src", "capabilities", "permissions"] {
        for path in files_under(&dir.join(folder)) {
            let relative = path
                .strip_prefix(dir)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            looked += 1;
            if allowed(&relative) {
                continue;
            }
            let lines: Vec<&str> = text.lines().filter(|line| names_it(line)).collect();
            if relative == "src/lib.rs" {
                assert_eq!(lines, ["mod delete_app_data;"], "{relative}");
            } else {
                assert!(lines.is_empty(), "{relative}: {lines:?}");
            }
        }
    }
    for file in ["build.rs", "tauri.conf.json"] {
        assert!(!read(file).lines().any(names_it), "{file}");
    }
    assert!(looked > 50, "only {looked} files read");
}
