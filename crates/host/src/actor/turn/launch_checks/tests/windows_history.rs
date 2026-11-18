//! On Windows a launch leaves this matter's `history.git` under the `matters/**` rules,
//! as before the history's rules, and Claude gets no history rules (`folders::own_folders`,
//! `data_fence::history_rules`): the Windows fallback for the release candidate's PowerShell
//! calls refused with no classifier verdict. Off Windows the history is kept and gets its rules.

use std::path::PathBuf;

use super::{folders, scratch};
use crate::agent::data_fence::{self, LaunchDeny};
use crate::write;

/// A Windows data folder, as the rules spell it.
const DATA: &str = r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop";

/// Another matter's id.
const OTHER: &str = "01a1d5b4-a291-7649-9fa4-000000000001";

/// The history options the history's deny rules name.
const OPTIONS: [&str; 4] = ["--no-index", "--output", "--exec", "--remote"];

#[test]
fn on_windows_the_history_stays_denied_and_gets_no_rules() {
    let data = scratch("windows-history");
    let stored = scratch("windows-history-matter");
    let run = folders(&data, 1);
    let history = crate::agent::history::matter_history(&run.notes).unwrap();
    let other = data.join("matters").join(OTHER).join("history.git");
    for file in [
        history.join("HEAD"),
        other.join("HEAD"),
        run.notes.join("n.md"),
    ] {
        write::create_dir_all(file.parent().unwrap()).unwrap();
        write::write_file(&file, b"x").unwrap();
    }
    let spelled = |deny: LaunchDeny| LaunchDeny {
        spellings: vec![PathBuf::from(DATA)],
        ..deny
    };
    let windows = spelled(super::super::folders::launch_deny(
        &run, &stored, &data, None, true,
    ));
    // The launch as it was before the history's rules: the same own folders, no `history.git`.
    let own = [
        run.root.clone(),
        run.work.clone(),
        run.notes.clone(),
        crate::agent::session_folder(&stored),
    ];
    let mut before = data_fence::launch_deny(vec![data.clone()], &data, &own, &[]);
    before.own_reads = data_fence::own_reads(&data, &[&run.work, &run.notes]);
    let before = spelled(before);
    assert_eq!(windows.targets, before.targets);
    assert_eq!(windows.kept, before.kept);
    assert!(windows.targets.iter().any(|target| target == "matters/**"));
    let shell = data_fence::windows_fixed_shell_deny_rules(&windows);
    assert_eq!(shell, data_fence::windows_fixed_shell_deny_rules(&before));
    let (allow, deny) = data_fence::history_rules(&windows, true);
    assert!(allow.is_empty() && deny.is_empty(), "{allow:?} {deny:?}");
    let mut rules = data_fence::claude_allow_rules_on(&windows, true);
    rules.extend(data_fence::claude_deny_rules_on(&windows, true, false));
    rules.extend(shell);
    for rule in &rules {
        assert!(!rule.contains("history.git"), "{rule}");
        assert!(
            !OPTIONS.iter().any(|option| rule.contains(option)),
            "{rule}"
        );
    }
    // Off Windows the history is kept, and its rules are made.
    let elsewhere = super::super::folders::launch_deny(&run, &stored, &data, None, false);
    assert!(
        elsewhere
            .kept
            .iter()
            .any(|place| place.ends_with("/history.git")),
        "{:?}",
        elsewhere.kept
    );
    let (allow, deny) = data_fence::history_rules(&elsewhere, false);
    assert!(!allow.is_empty() && !deny.is_empty());
    for folder in [&data, &stored] {
        write::remove_dir_all(folder).unwrap();
    }
}
