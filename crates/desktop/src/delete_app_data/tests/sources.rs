//! What the sources must keep: the uninstall hook runs `--delete-app-data` and clears the box
//! before the template's `RmDir /r`, with the exit codes and words it names; and nothing a window
//! can reach (commands, the bridge, capabilities, permissions) names the deletion.

use std::path::{Path, PathBuf};

use crate::delete_app_data::{FLAG, KEEP_SANDBOX, KEPT_SANDBOX, KEPT_SET_ASIDE};

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

#[test]
fn the_uninstall_hook_runs_the_deletion_after_s15s_removal_and_clears_the_box_first() {
    let code = hook_code();
    let start = line_with(&code, &["!macro NSIS_HOOK_PREUNINSTALL"]);
    let end = start + line_with(&code[start..], &["!macroend"]);
    let body = &code[start..end];
    let remove = line_with(body, &["clauseo-sandbox-setup.exe", "--remove"]);
    let cleared = line_with(body, &["StrCpy $DeleteAppDataCheckboxState 0"]);
    let plain = format!("\"$INSTDIR\\${{MAINBINARYNAME}}.exe\" {FLAG}'");
    let keeping = format!("\"$INSTDIR\\${{MAINBINARYNAME}}.exe\" {FLAG} {KEEP_SANDBOX}'");
    let runs = line_with(body, &["ExecWait", &plain]);
    let runs_keeping = line_with(body, &["ExecWait", &keeping]);
    assert!(
        remove < cleared,
        "S15's --remove runs before anything is deleted"
    );
    assert!(
        cleared < runs.min(runs_keeping),
        "the box is cleared before the app runs"
    );
    // The box cleared once and never set again; nothing in the file deletes a tree itself.
    assert_eq!(
        code.iter()
            .filter(|line| line.contains("$DeleteAppDataCheckboxState"))
            .count(),
        2,
        "one test of the box and one clearing: {body:?}"
    );
    for line in &code {
        let lower = line.to_lowercase();
        assert!(!lower.contains("rmdir"), "{line}");
        assert!(
            !lower.contains("rd /s") && !lower.contains("rmdir /s"),
            "{line}"
        );
    }
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
    for key in ["deleteAppDataFailed", "deleteAppDataKept"] {
        line_with(
            &code,
            &["MessageBox MB_OK|MB_ICONEXCLAMATION", &format!("$({key})")],
        );
        assert!(
            english.contains(&format!("LangString {key} ${{LANG_ENGLISH}} \"")),
            "{key}"
        );
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

/// Whether `line` names the module (a path to it, or its declaration) or the flag.
fn names_it(line: &str) -> bool {
    [
        "::delete_app_data",
        "delete_app_data::",
        "mod delete_app_data",
        FLAG,
    ]
    .iter()
    .any(|name| line.contains(name))
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
        let text = read(file);
        assert!(
            !text.contains("delete_app_data") && !text.contains(FLAG),
            "{file}"
        );
    }
    assert!(looked > 50, "only {looked} files read");
}
