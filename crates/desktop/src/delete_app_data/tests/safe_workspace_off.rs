//! The Safe workspace is off for launch (decision 0035): with `CLAUSEO_SAFE_WORKSPACE` undefined,
//! the release default, the hooks compile without the elevated set-up and removal. Neither
//! installer starts anything elevated (no `runas`, no `ShellExecuteExW`), writes or runs the
//! set-up program, or touches the machine's Sandbox record, and the uninstaller never starts the
//! removal. With the define (`hooks_safe_workspace.nsh`) all of it is back, which the other
//! tests cover by defining it.

use std::collections::{BTreeSet, HashMap};

use super::admin_copy::{admin_code, code_of, macro_body};
use super::nsis_run::{defines, preprocess};
use super::sources::{hook_code, line_with, preuninstall, read};

/// The define that turns the Safe workspace on (`hooks.nsh`, `hooks_safe_workspace.nsh`).
pub(super) const SAFE_WORKSPACE: &str = "CLAUSEO_SAFE_WORKSPACE";

const MODES: [&str; 2] = ["currentUser", "perMachine"];

/// What the template inserts from the hooks: its sections' and callbacks' macros.
const INSERTED: [&str; 9] = [
    "NSIS_HOOK_POSTINSTALL",
    "NSIS_HOOK_PREUNINSTALL",
    "CLAUSEO_MACHINE_START",
    "CLAUSEO_UNINSTALL_IN_PLACE_CHECK",
    "CLAUSEO_UNINSTALL_SELF_DELETE",
    "CLAUSEO_UNINSTALL_GUI_END",
    "CLAUSEO_UI_LANGUAGE",
    "CLAUSEO_ELEVATED_BRANCH",
    "CLAUSEO_FINISH_SHOW",
];

/// The defines the `mode` installer compiles `code` with, the Safe workspace's when `on`.
fn mode_defines(code: &[String], mode: &str, on: bool) -> HashMap<String, String> {
    let mut defines = defines(code);
    defines.insert("INSTALLMODE".to_owned(), mode.to_owned());
    defines.insert("NSIS_PTR_SIZE".to_owned(), "4".to_owned());
    defines.insert("ARCH".to_owned(), "x64".to_owned());
    defines.insert("LANG_HINDI".to_owned(), "1081".to_owned());
    if on {
        defines.insert(SAFE_WORKSPACE.to_owned(), String::new());
    }
    defines
}

/// The files `hooks.nsh` includes at file scope in the `mode` build.
fn includes(mode: &str, on: bool) -> Vec<String> {
    let code = hook_code();
    preprocess(&code, &mode_defines(&code, mode, on))
        .into_iter()
        .filter_map(|line| {
            let rest = line.strip_prefix("!include \"${__FILEDIR__}\\")?;
            Some(rest.trim_end_matches('"').to_owned())
        })
        .collect()
}

/// The `mode` build's code: `hooks.nsh` and the files it includes, as compiled.
fn compiled(mode: &str, on: bool) -> Vec<String> {
    let mut code = hook_code();
    for file in includes(mode, on) {
        if file == "admin_copy.nsh" {
            code.extend(admin_code());
            code.extend(code_of("windows/uninstall_in_place.nsh"));
        } else {
            code.extend(code_of(&format!("windows/{file}")));
        }
    }
    let defines = mode_defines(&code, mode, on);
    preprocess(&code, &defines)
}

/// Whether `code` defines the macro `name`.
fn has_macro(code: &[String], name: &str) -> bool {
    let open = format!("!macro {name}");
    code.iter()
        .any(|line| *line == open || line.starts_with(&format!("{open} ")))
}

/// Every line the template's inserts reach in `code`, through each `!insertmacro` of a macro
/// `code` defines (the template's own, such as CheckIfAppIsRunning, are not the hooks').
fn reached(code: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut todo: Vec<String> = INSERTED.iter().map(|name| (*name).to_owned()).collect();
    let mut out = Vec::new();
    while let Some(next) = todo.pop() {
        if !has_macro(code, &next) || !seen.insert(next.clone()) {
            continue;
        }
        for line in macro_body(code, &next) {
            if let Some(rest) = line.strip_prefix("!insertmacro ") {
                todo.push(rest.split_whitespace().next().unwrap().to_owned());
            }
            out.push(line.clone());
        }
    }
    out
}

/// The lines of `lines` that elevate, run or write the set-up, or touch its record.
fn safe_workspace_lines(lines: &[String]) -> Vec<String> {
    let banned = [
        "runas",
        "ShellExecute",
        "sandbox-setup",
        "--adopt",
        "--remove",
        "CLAUSEO_ADMIN_EXE",
        "CLAUSEO_ADMIN_DIR",
        "CLAUSEO_RECORD_KEY",
        "Clauseo\\Sandbox",
    ];
    lines
        .iter()
        .filter(|line| {
            banned.iter().any(|word| line.contains(word))
                || (line.starts_with("WriteReg") || line.starts_with("DeleteReg"))
                    && line.contains("HKLM")
        })
        .cloned()
        .collect()
}

#[test]
fn off_hooks_nsh_includes_only_the_temp_folder_and_the_in_place_uninstall() {
    for mode in MODES {
        assert_eq!(
            includes(mode, false),
            ["safe_temp.nsh", "uninstall_in_place.nsh"],
            "{mode}"
        );
        // On, admin_copy.nsh, which includes those two and the elevated branch itself.
        assert_eq!(includes(mode, true), ["admin_copy.nsh"], "{mode}");
    }
    let copy = code_of("windows/admin_copy.nsh");
    for file in [
        "safe_temp.nsh",
        "uninstall_in_place.nsh",
        "admin_elevated.nsh",
    ] {
        line_with(&copy, &[&format!("!include \"${{__FILEDIR__}}\\{file}\"")]);
    }
}

#[test]
fn off_neither_installer_elevates_runs_the_set_up_or_writes_its_record() {
    for mode in MODES {
        let off = compiled(mode, false);
        // The elevated branch, the ask, the per-machine set-up and the finish page's box are not
        // compiled, so the template's `!ifmacrodef` blocks insert nothing.
        for name in [
            "CLAUSEO_ELEVATED_BRANCH",
            "CLAUSEO_ASK_FOR_THE_ADMIN_SECTION",
            "CLAUSEO_ADMIN_COPY_MACHINE",
            "CLAUSEO_FINISH_SHOW",
        ] {
            assert!(!has_macro(&off, name), "{mode}: {name}");
        }
        assert!(
            off.iter()
                .all(|line| !line.contains("ClauseoAdminTimedOut"))
        );
        let lines = reached(&off);
        assert!(
            lines.len() > 50,
            "{mode}: only {} lines reached",
            lines.len()
        );
        assert_eq!(safe_workspace_lines(&lines), Vec::<String>::new(), "{mode}");
        // The firm-policy folder stays per machine, and the language per mode.
        let post = macro_body(&off, "NSIS_HOOK_POSTINSTALL");
        let policy = post.iter().any(|line| line.contains("policyFolderExists"));
        assert_eq!(policy, mode == "perMachine", "{mode}");
        line_with(
            post,
            &["WriteRegStr HKCU \"${MANUPRODUCTKEY}\" \"Installer Language\""],
        );
        // On, the same check finds the set-up: it would catch the code coming back unseen.
        let on = reached(&compiled(mode, true));
        let found = safe_workspace_lines(&on).join("\n");
        assert!(found.contains("ShellExecuteExW"), "{mode}: {found}");
        assert!(found.contains("--adopt"), "{mode}: {found}");
        let elevated = if mode == "currentUser" {
            "w \"runas\""
        } else {
            "--per-machine"
        };
        assert!(found.contains(elevated), "{mode}: {found}");
    }
}

#[test]
fn off_the_uninstaller_never_starts_the_removal_and_keeps_the_sandbox_folder() {
    for mode in MODES {
        let code = hook_code();
        let off = preprocess(preuninstall(&code), &mode_defines(&code, mode, false));
        assert_eq!(safe_workspace_lines(&off), Vec::<String>::new(), "{mode}");
        for gone in [
            "ShellExecuteExW",
            "WaitForSingleObject",
            "CLAUSEO_OWNER_TO_TELL",
        ] {
            assert!(
                off.iter().all(|line| !line.contains(gone)),
                "{mode}: {gone}"
            );
        }
        // $R5 stays 0, so the app is never told the sandbox was removed (it keeps `sandbox\`).
        line_with(&off, &["StrCpy $R5 0"]);
        assert!(off.iter().all(|line| line != "StrCpy $R5 1"), "{mode}");
        // The rest of the uninstall stays: the app check first, then the data deletion.
        let check = line_with(&off, &["!insertmacro CheckIfAppIsRunning"]);
        let delete = line_with(&off, &["--delete-app-data' $R7"]);
        assert!(check < delete, "{mode}");
        // On, the removal is there, started elevated per user.
        let on = preprocess(preuninstall(&code), &mode_defines(&code, mode, true));
        line_with(&on, &["shell32::ShellExecuteExW(p R6)"]);
        line_with(&on, &["StrCpy $R5 1"]);
    }
}

#[test]
fn the_release_default_is_off_and_hooks_safe_workspace_nsh_turns_it_on() {
    // windows.conf.json, which every Windows release build passes, names the plain hooks.
    let conf: serde_json::Value = serde_json::from_str(&read("release/windows.conf.json")).unwrap();
    assert_eq!(
        conf["bundle"]["windows"]["nsis"]["installerHooks"],
        "windows/hooks.nsh"
    );
    // hooks.nsh never defines it itself; hooks_safe_workspace.nsh defines it, then includes it.
    let hooks = read("windows/hooks.nsh");
    assert!(
        hooks.lines().all(|line| !line
            .trim()
            .starts_with(&format!("!define {SAFE_WORKSPACE}"))),
        "hooks.nsh defines {SAFE_WORKSPACE}"
    );
    let on = code_of("windows/hooks_safe_workspace.nsh");
    assert_eq!(
        on,
        [
            format!("!define {SAFE_WORKSPACE}"),
            "!include \"${__FILEDIR__}\\hooks.nsh\"".to_owned(),
        ]
    );
    // release.yml's switch names that file only when on (latest-json.test.mjs checks the rest).
    let release = read("../../.github/workflows/release.yml");
    assert!(release.contains("  SAFE_WORKSPACE: \"false\"\n"));
    assert!(release.contains("windows/hooks_safe_workspace.nsh"));
}
