//! What the sources must keep: the uninstall hook closes the app, runs S15's removal, then runs
//! `--delete-app-data` with the box cleared before the template's `RmDir /r`, with the exit codes,
//! folder names and words it names; and nothing a window can reach (commands, the bridge,
//! capabilities, permissions) names the deletion.

use std::path::{Path, PathBuf};

use crate::delete_app_data::walk::{SANDBOX_DIR, SET_ASIDE_PATH};
use crate::delete_app_data::{FLAG, KEPT_SANDBOX, KEPT_SET_ASIDE, SANDBOX_REMOVED};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(manifest_dir().join(relative)).unwrap()
}

/// `hooks.nsh`'s lines, comments and blank lines left out, trimmed.
fn hook_code() -> Vec<String> {
    read("windows/hooks.nsh")
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with(';'))
        .map(str::to_owned)
        .collect()
}

/// The index of the first code line holding all of `parts`.
fn line_with(code: &[String], parts: &[&str]) -> usize {
    code.iter()
        .position(|line| parts.iter().all(|part| line.contains(part)))
        .unwrap_or_else(|| panic!("no line in hooks.nsh holds {parts:?}"))
}

/// The pre-uninstall macro's code lines.
fn preuninstall(code: &[String]) -> &[String] {
    let start = line_with(code, &["!macro NSIS_HOOK_PREUNINSTALL"]);
    let end = start + line_with(&code[start..], &["!macroend"]);
    &code[start..end]
}

#[test]
fn the_uninstall_hook_checks_the_app_then_removes_s15s_set_up_then_runs_the_deletion() {
    let code = hook_code();
    let body = preuninstall(&code);
    let app_check = line_with(body, &["!insertmacro CheckIfAppIsRunning"]);
    let remove = line_with(body, &["clauseo-sandbox-setup.exe", "--remove"]);
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
    line_with(
        body,
        &[&format!("${{FileExists}} \"$R9\\{SANDBOX_DIR}\\*.*\"")],
    );
    line_with(body, &[&format!("StrCpy $R8 \"$R9\\{SANDBOX_DIR}\"")]);
    // The Kept line whenever a folder stays, the Failed line only when none does.
    let kept = line_with(body, &["$(deleteAppDataKept)"]);
    let failed = line_with(body, &["$(deleteAppDataFailed)"]);
    assert_eq!(body[kept - 1], "${If} $R8 != \"\"");
    assert_eq!(body[failed - 1], "${Else}");
}

#[test]
fn the_hooks_exit_codes_and_words_are_the_apps() {
    let code = hook_code();
    for (name, value) in [
        ("CLAUSEO_DELETED", super::DELETED),
        ("CLAUSEO_KEPT_SANDBOX", KEPT_SANDBOX),
        ("CLAUSEO_KEPT_SET_ASIDE", KEPT_SET_ASIDE),
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
