//! The Windows dry run of 2026-11-11: Claude, in Git Bash, ran
//! `powershell -NoProfile -Command "\$env:Path -split ';'"` (the `$` escaped from Bash), and the
//! fence refused it unseen. With Windows' rules `\` is no escape, so `\$env:Path` was a path
//! word whose name `$env:Path` holds a `:`, refused untouched as an alternate data stream.
//! PowerShell's variable `$env:NAME` is judged by the file Windows would open for that text,
//! `$env`: no stream of a file outside the data folder reaches it, and one inside still does.
//! PowerShell expands the variable, so where it leads is judged too (`variables.rs`): the
//! 2026-11-11 review's commands, a base or a search folder under a variable, are refused.

#[cfg(unix)]
use std::path::PathBuf;

#[cfg(unix)]
use serde_json::json;

#[cfg(unix)]
use super::{DataFenceHealth, FenceCheck, candidates, candidates_roots, candidates_words};

/// Session `s1`'s check of `command` from the matter `/t/matter`, the data folder `/t/data`
/// beside it, read with Windows' rules, as `fence_check` builds it.
#[cfg(unix)]
fn shell(command: &str) -> FenceCheck {
    let raw = json!({ "command": command, "description": "List Windows PATH entries" });
    FenceCheck {
        data: PathBuf::from("/t/data"),
        allowed: vec![PathBuf::from("/t/data/run/s1")],
        cwd: PathBuf::from("/t/matter"),
        home: None,
        paths: candidates(&[], Some(&raw), true),
        roots: candidates_roots(Some(&raw), true),
        words: candidates_words(Some(&raw), true),
        health: DataFenceHealth::default(),
        vars: Vec::new(),
    }
}

#[cfg(unix)]
fn refused_on(command: &str, names: &[(&str, crate::network_paths::resolve::tests::Fake)]) -> bool {
    use super::network_tests::refused;
    use crate::network_paths::resolve::tests::FakeFs;
    use crate::network_paths::tests::plain_net;

    refused(&shell(command), &plain_net(true), &FakeFs::with(names))
}

/// The dry run's three commands, in each way Claude writes the third, pass.
#[cfg(unix)]
#[test]
fn reading_path_through_powershell_env_is_not_refused() {
    use crate::network_paths::resolve::tests::Fake;

    let names = [
        ("/t/data/state.sqlite", Fake::File),
        ("/t/matter/a.txt", Fake::File),
    ];
    for command in [
        "stetdoc --version",
        "echo $PATH",
        r#"powershell -NoProfile -Command "$env:Path -split ';'""#,
        r#"powershell -NoProfile -Command "\$env:Path -split ';'""#,
        r#"powershell.exe -NoProfile -Command '$env:Path -split ";"'"#,
        r#"powershell -NoProfile -Command "\$env:PATH.Split(';')""#,
        r#"pwsh -NoProfile -Command "\${env:Path} -split ';'""#,
    ] {
        assert!(!refused_on(command, &names), "{command}");
    }
}

/// What the guarantee keeps: a stream named like the variable on a file in the data folder, or
/// on a link into it, the data folder named beside the variable, and any other stream name.
#[cfg(unix)]
#[test]
fn a_variable_shaped_stream_into_the_data_folder_is_still_refused() {
    use crate::network_paths::resolve::tests::Fake;

    let names = [
        ("/t/data/state.sqlite", Fake::File),
        ("/t/data/$env", Fake::File),
        ("/t/matter/$env", Fake::Link("/t/data".into())),
        ("/t/matter/a.txt", Fake::File),
    ];
    for command in [
        r#"powershell -Command "Get-Content /t/data/$env:Path""#,
        r"type /t/data/$env:Path",
        r"type /t/matter/$env:Path",
        r#"powershell -Command "\$env:Path; type D:/t/data/state.sqlite""#,
        r"type /t/matter/a.txt:stream",
        r"type /t/matter/$env:Path:$DATA",
        r"type /t/matter/$env:",
        r"type /t/matter/b:Path",
        r"type /t/matter/env:Path",
    ] {
        assert!(refused_on(command, &names), "{command}");
    }
}

/// The review's layout, as on Windows: the home `/u`, the data folder in its local app data,
/// the matter under its documents, the agent's `TEMP` in the data folder's run folder, and
/// `CFG` naming a file in the data folder.
#[cfg(unix)]
fn under_home(command: &str) -> FenceCheck {
    let raw = json!({ "command": command });
    let vars = [
        ("LOCALAPPDATA", "/u/AppData/Local"),
        ("TEMP", "/u/AppData/Local/clauseo/run/s1/tmp"),
        ("CFG", "/u/AppData/Local/clauseo/state.sqlite"),
    ];
    FenceCheck {
        data: PathBuf::from("/u/AppData/Local/clauseo"),
        allowed: vec![PathBuf::from("/u/AppData/Local/clauseo/run/s1")],
        cwd: PathBuf::from("/u/Documents/matter"),
        home: Some(PathBuf::from("/u")),
        paths: candidates(&[], Some(&raw), true),
        roots: candidates_roots(Some(&raw), true),
        words: candidates_words(Some(&raw), true),
        health: DataFenceHealth::default(),
        vars: vars.map(|(n, v)| (n.to_owned(), v.to_owned())).to_vec(),
    }
}

/// [`under_home`]'s files.
#[cfg(unix)]
fn under_home_refused(command: &str) -> bool {
    use super::network_tests::refused;
    use crate::network_paths::resolve::tests::{Fake, FakeFs};
    use crate::network_paths::tests::plain_net;

    let names = [
        ("/u/AppData/Local/clauseo/state.sqlite", Fake::File),
        ("/u/AppData/Local/clauseo/run/s1/tmp/a.txt", Fake::File),
        ("/u/Documents/matter/a.txt", Fake::File),
        ("/u/Documents/x", Fake::File),
    ];
    refused(
        &under_home(command),
        &plain_net(true),
        &FakeFs::with(&names),
    )
}

/// A variable as a search folder or a path's base is judged where PowerShell takes it.
#[cfg(unix)]
#[test]
fn a_variable_leading_into_the_data_folder_is_refused() {
    for command in [
        "Get-ChildItem -Recurse $env:USERPROFILE",
        "Select-String -Path $env:LOCALAPPDATA/* -Pattern secret",
        "type $env:TEMP/../../../state.sqlite",
        "type $env:USERPROFILE/AppData/Local/./clauseo/state.sqlite",
        "type ${env:LOCALAPPDATA}/./clauseo/state.sqlite",
        "type $env:CFG",
        "Get-ChildItem -Recurse $env:NOT_SET_ANYWHERE",
    ] {
        assert!(under_home_refused(command), "{command}");
    }
}

/// A variable that leads where the agent may go passes: the matter under the home, the
/// agent's own temp folder, and the dry run's list.
#[cfg(unix)]
#[test]
fn a_variable_leading_inside_the_matter_or_the_own_folders_passes() {
    for command in [
        "type $env:USERPROFILE/Documents/matter/a.txt",
        "type $env:TEMP/a.txt",
        r#"powershell -NoProfile -Command "\$env:Path -split ';'""#,
    ] {
        assert!(!under_home_refused(command), "{command}");
    }
}
