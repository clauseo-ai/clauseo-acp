//! The exact `Edit` denies of the folders above other matters (`ancestor_edit_rules`): only
//! those that hold neither the own matter nor the data folder, never a drive root, so the own
//! matter's files stay writable.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::{LaunchDeny, claude_deny_rules_on};
use crate::agent::matter_fence::{MatterFence, Other};

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// `C:\Clients\P\{A,B}` and `C:\Clients\Q\C`, the conversation in `A`; `B` invited when
/// `invited`.
fn launch(others: &[(&str, bool)], own: &[&str]) -> LaunchDeny {
    let others = others
        .iter()
        .zip(1..)
        .map(|((folder, invited), n)| {
            let folder = PathBuf::from(folder);
            let mut other = Other::new(id(n), format!("M{n}"), folder.clone(), *invited);
            other.spellings = vec![folder.clone()];
            other.paths = vec![folder];
            other
        })
        .collect();
    let mut fence = MatterFence::asked(others);
    fence.own = own.iter().map(PathBuf::from).collect();
    let mut deny = LaunchDeny::fixed(vec![PathBuf::from(
        r"C:\Users\Asha\AppData\Local\ai.clauseo.desktop",
    )]);
    deny.matters = fence;
    deny
}

/// Whether any `Edit` rule of `rules` covers `path` (a rule path, `//c/…`), read as
/// gitignore reads it: the path itself and all under it, or with `/**` all under it.
fn edit_covers(rules: &[String], path: &str) -> Option<String> {
    let path = path.to_lowercase();
    rules
        .iter()
        .filter_map(|rule| Some((rule, rule.strip_prefix("Edit(")?.strip_suffix(')')?)))
        .find(|(_, at)| {
            let at = at.to_lowercase();
            let base = at.strip_suffix("/**").unwrap_or(&at);
            path == base || path.starts_with(&format!("{base}/"))
        })
        .map(|(rule, _)| rule.clone())
}

#[test]
fn a_folder_above_other_matters_alone_is_denied_exactly() {
    let deny = launch(
        &[(r"C:\Clients\P\B", false), (r"C:\Clients\Q\C", false)],
        &[r"C:\Clients\P\A"],
    );
    let rules = claude_deny_rules_on(&deny, true, false);
    assert!(
        rules.contains(&"Edit(//c/Clients/Q)".to_owned()),
        "{rules:?}"
    );
    for absent in [
        "Edit(//c/Clients)",
        "Edit(//c/Clients/P)",
        "Edit(//c)",
        "Edit(//c/)",
        "Edit(//c/Clients/Q/**)",
        "Edit(//c/Clients/**)",
    ] {
        assert!(!rules.contains(&absent.to_owned()), "{absent}: {rules:?}");
    }
    // The own matter's files stay writable; the others' are not.
    for own in [
        "//c/Clients/P/A/x.docx",
        "//c/Clients/P/A",
        "//c/clients/p/a/sub/y.txt",
    ] {
        assert_eq!(edit_covers(&rules, own), None, "{own}");
    }
    for other in [
        "//c/Clients/P/B/f.txt",
        "//c/Clients/Q/C/f.txt",
        "//c/Clients/Q",
    ] {
        assert!(edit_covers(&rules, other).is_some(), "{other}");
    }
}

/// An invited matter's folders above it are denied too (an invite is read-only); a folder above
/// the data folder never is (it holds the session's own run folder), nor any before the launch
/// resolved the own matter.
#[test]
fn invited_matters_count_and_the_data_folders_parents_never_do() {
    let deny = launch(
        &[
            (r"C:\Clients\Q\C", true),
            (r"C:\Users\Asha\Matters\D", false),
        ],
        &[r"C:\Clients\P\A"],
    );
    let rules = claude_deny_rules_on(&deny, true, false);
    assert!(
        rules.contains(&"Edit(//c/Clients/Q)".to_owned()),
        "{rules:?}"
    );
    assert!(
        rules.contains(&"Edit(//c/Users/Asha/Matters)".to_owned()),
        "{rules:?}"
    );
    for absent in [
        "Edit(//c/Users/Asha)",
        "Edit(//c/Users)",
        "Edit(//c/Clients)",
    ] {
        assert!(!rules.contains(&absent.to_owned()), "{absent}: {rules:?}");
    }
    let data = "//c/Users/Asha/AppData/Local/ai.clauseo.desktop/run/own/work/x.md";
    assert_eq!(
        edit_covers(&rules, data).filter(|r| !r.contains("ai.clauseo")),
        None
    );
    // Each spelling once, across matters: two matters under `Q` give one rule.
    let twice = launch(
        &[(r"C:\Clients\Q\C", false), (r"C:\Clients\Q\E", false)],
        &[r"C:\Clients\P\A"],
    );
    let rules = claude_deny_rules_on(&twice, true, false);
    assert_eq!(
        rules.iter().filter(|r| *r == "Edit(//c/Clients/Q)").count(),
        1
    );
    let unresolved = launch(&[(r"C:\Clients\Q\C", false)], &[]);
    let rules = claude_deny_rules_on(&unresolved, true, false);
    assert!(
        !rules.contains(&"Edit(//c/Clients/Q)".to_owned()),
        "{rules:?}"
    );
}

/// The own matter's 8.3 spelling keeps the short folders above it out too.
#[test]
fn the_own_matters_short_spelling_counts() {
    let deny = launch(
        &[(r"C:\CLIENT~1\Q\C", false), (r"C:\Clients\Q\C", false)],
        &[r"C:\Clients\P\A", r"C:\CLIENT~1\P\A"],
    );
    let rules = claude_deny_rules_on(&deny, true, false);
    assert!(
        rules.contains(&"Edit(//c/CLIENT~1/Q)".to_owned()),
        "{rules:?}"
    );
    assert!(
        !rules.contains(&"Edit(//c/CLIENT~1)".to_owned()),
        "{rules:?}"
    );
    assert_eq!(edit_covers(&rules, "//c/client~1/p/a/x.docx"), None);
}
