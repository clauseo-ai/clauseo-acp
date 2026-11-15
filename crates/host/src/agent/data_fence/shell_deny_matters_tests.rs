//! Claude's Windows shell rules for other matters, matched as Claude Code 2.1.293 matches them
//! (`super::super::tests::denied_by`): each spelling of a fence path is denied, the session's
//! own matter is not, a name boundary keeps `Smith 2` open beside a fenced `Smith`, and an
//! invited matter keeps its rules.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::super::cross_tests::env;
use super::super::tests::denied_by;
use super::windows_matter_shell_rules;
use crate::agent::data_fence::LaunchDeny;
use crate::agent::matter_fence::{MatterFence, Other};

const MATTERS: &str = r"C:\Users\clauseo-lawyer\Matters";

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// A launch in `MATTERS\<own>` fencing `others` (name, fence paths below `MATTERS`, invited).
fn launch(own: &str, others: &[(&str, &[&str], bool)]) -> LaunchDeny {
    let at = |name: &str| PathBuf::from(format!(r"{MATTERS}\{name}"));
    let others = others
        .iter()
        .zip(1..)
        .map(|((name, paths, invited), n)| {
            let mut other = Other::new(id(n), (*name).to_owned(), at(name), *invited);
            other.spellings = vec![at(name)];
            other.paths = paths.iter().map(|path| at(path)).collect();
            other
        })
        .collect();
    let mut matters = MatterFence::asked(others);
    matters.own = vec![at(own)];
    LaunchDeny {
        spellings: vec![PathBuf::from(
            r"C:\Users\clauseo-lawyer\AppData\Local\ai.clauseo.desktop",
        )],
        matters,
        ..LaunchDeny::default()
    }
}

fn all_rules(deny: &LaunchDeny) -> Vec<String> {
    windows_matter_shell_rules(deny, &env, true)
        .into_iter()
        .flat_map(|(_, rules)| rules)
        .collect()
}

/// Whether both shells deny `command`.
fn denied(rules: &[String], command: &str) -> bool {
    let bash = denied_by(rules, "Bash", command);
    let powershell = denied_by(rules, "PowerShell", command);
    assert_eq!(bash, powershell, "{command}");
    bash
}

#[test]
fn a_sibling_matter_is_denied_in_each_spelling_and_the_own_matter_is_not() {
    let rules = all_rules(&launch("Jones", &[("Smith", &["Smith"], false)]));
    for command in [
        r#"type "C:\Users\clauseo-lawyer\Matters\Smith\lease.pdf""#,
        r"cat C:/Users/clauseo-lawyer/Matters/Smith/lease.pdf",
        r"dir C:\Users\clauseo-lawyer\Matters\Smith",
        r"copy %USERPROFILE%\Matters\Smith\a.docx .",
        r"cat ~/Matters/Smith/a.docx",
    ] {
        assert!(denied(&rules, command), "{command}: {rules:#?}");
    }
    assert!(denied_by(
        &rules,
        "PowerShell",
        r"Get-Content $env:USERPROFILE\Matters\Smith\a"
    ));
    assert!(denied_by(
        &rules,
        "Bash",
        r"cat /c/Users/clauseo-lawyer/Matters/Smith/a"
    ));
    for command in [
        r"type C:\Users\clauseo-lawyer\Matters\Jones\lease.pdf",
        r"dir C:\Users\clauseo-lawyer\Matters",
    ] {
        assert!(!denied(&rules, command), "{command}: {rules:#?}");
    }
}

#[test]
fn a_fenced_name_the_own_matter_starts_with_ends_at_a_name_boundary() {
    let rules = all_rules(&launch("Smith 2", &[("Smith", &["Smith"], false)]));
    assert!(denied(
        &rules,
        r#"type "C:\Users\clauseo-lawyer\Matters\Smith\a.pdf""#
    ));
    assert!(denied(&rules, r"dir C:\Users\clauseo-lawyer\Matters\Smith"));
    assert!(!denied(
        &rules,
        r#"type "C:\Users\clauseo-lawyer\Matters\Smith 2\a.pdf""#
    ));
}

#[test]
fn a_parent_is_denied_by_its_siblings_and_an_invite_keeps_its_rules() {
    let deny = launch(
        r"Acme\Deal",
        &[
            (r"Acme", &[r"Acme\Old", r"Acme\notes.txt"], false),
            ("Zed", &["Zed"], true),
        ],
    );
    let lists = windows_matter_shell_rules(&deny, &env, true);
    assert_eq!(lists.len(), 2, "one list each, the invited matter's too");
    let rules: Vec<String> = lists.into_iter().flat_map(|(_, rules)| rules).collect();
    for command in [
        r"type C:\Users\clauseo-lawyer\Matters\Acme\Old\memo.docx",
        r"type C:\Users\clauseo-lawyer\Matters\Acme\notes.txt",
        r"type C:\Users\clauseo-lawyer\Matters\Zed\memo.docx",
    ] {
        assert!(denied(&rules, command), "{command}");
    }
    assert!(!denied(
        &rules,
        r"type C:\Users\clauseo-lawyer\Matters\Acme\Deal\memo.docx"
    ));
}

#[test]
fn without_variables_the_lists_are_shorter_and_still_deny_the_path() {
    let deny = launch("Jones", &[("Smith", &["Smith"], false)]);
    let with = all_rules(&deny);
    let without: Vec<String> = windows_matter_shell_rules(&deny, &env, false)
        .into_iter()
        .flat_map(|(_, rules)| rules)
        .collect();
    assert!(without.len() < with.len(), "{without:#?}");
    assert!(denied(
        &without,
        r"type C:\Users\clauseo-lawyer\Matters\Smith\a"
    ));
}
