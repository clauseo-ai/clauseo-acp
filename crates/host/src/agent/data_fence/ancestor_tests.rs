//! No folder above another matter is in an `Edit` deny: one covered everything inside it, so it
//! refused an ordinary save (a copy to Documents while another matter lives there) and, for a
//! matter under `/tmp` or `/var`, every shell command (Claude Code's sandbox keeps its working
//! folder's file in `/tmp/clsb-…`). The other matters themselves stay denied.

use std::path::PathBuf;

use clauseo_protocol::MatterId;

use super::{LaunchDeny, claude_deny_rules_on, rule_path};
use crate::agent::matter_fence::{MatterFence, Other};

fn id(n: u128) -> MatterId {
    let bits = (0x0190_0000_0000_u128 << 80) | (0x7 << 76) | (0b10 << 62) | n;
    MatterId::new(uuid::Uuid::from_u128(bits)).unwrap()
}

/// A launch whose other matters are `others` (folder, invited), each fenced whole, and whose
/// own matter is spelled `own`.
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
        r"C:\Users\l\AppData\Local\ai.clauseo.desktop",
    )]);
    deny.matters = fence;
    deny
}

/// Whether any `Edit` rule of `rules` covers the rule path `path` (`//c/…`), read as gitignore
/// reads it: the path itself and all under it, or with `/**` all under it.
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

/// For each launch (`windows`: Windows' rule form), no folder above an other matter's folder is
/// covered by an `Edit` rule, and each other matter's files are.
fn assert_only_the_matters(deny: &LaunchDeny, windows: bool, above: &[&str], files: &[&str]) {
    let rules = claude_deny_rules_on(deny, windows, false);
    for folder in above {
        let at = rule_path(folder, windows);
        assert_eq!(edit_covers(&rules, &at), None, "{folder}: {rules:#?}");
        let saved = format!("{at}/copy of the brief.docx");
        assert_eq!(edit_covers(&rules, &saved), None, "{saved}: {rules:#?}");
    }
    for file in files {
        let at = rule_path(file, windows);
        assert!(edit_covers(&rules, &at).is_some(), "{file}: {rules:#?}");
    }
}

/// The own matter on the Desktop, another under Documents: nothing in `Documents` or
/// `Documents/Clients` is denied but the other matter, so a copy saved to Documents goes.
#[cfg(unix)]
#[test]
fn a_folder_above_another_matter_gets_no_edit_rule() {
    let deny = launch(
        &[
            ("/Users/l/Documents/Clients/B", false),
            ("/Users/l/Documents/C", true),
        ],
        &["/Users/l/Desktop/Deal"],
    );
    assert_only_the_matters(
        &deny,
        false,
        &[
            "/Users/l/Documents",
            "/Users/l/Documents/Clients",
            "/Users/l",
            "/Users",
            "/",
        ],
        &[
            "/Users/l/Documents/Clients/B/f.txt",
            "/Users/l/Documents/C/f.txt",
        ],
    );
}

/// Matters under `/tmp` and `/var/folders`: no `Edit` of `/tmp`, `/private/tmp`, `/var`,
/// `/private/var` or any folder down to the matters, so Claude Code's sandbox temp folder
/// (`/tmp/clsb-…`) stays writable.
#[cfg(unix)]
#[test]
fn matters_under_tmp_and_var_leave_the_temp_folders_writable() {
    let deny = launch(
        &[
            ("/tmp/x/B", false),
            ("/private/tmp/y/B", false),
            ("/private/var/folders/ab/cd/T/attach/B", false),
            ("/var/folders/ab/cd/T/B", false),
        ],
        &["/Users/l/Desktop/Deal"],
    );
    assert_only_the_matters(
        &deny,
        false,
        &[
            "/tmp",
            "/tmp/x",
            "/tmp/clsb-abcd1234/claude-501",
            "/private/tmp",
            "/var",
            "/private/var",
            "/var/folders",
            "/private/var/folders",
            "/private/var/folders/ab/cd/T/attach",
        ],
        &["/tmp/x/B/f", "/private/var/folders/ab/cd/T/attach/B/f"],
    );
}

/// The same on Windows: `C:\Users\l\Documents` and the folders down to another matter get no
/// rule, its 8.3 spelling neither.
#[test]
fn a_windows_folder_above_another_matter_gets_no_edit_rule() {
    let deny = launch(
        &[
            (r"C:\Users\l\Documents\Clients\B", false),
            (r"C:\Users\l\DOCUME~1\Clients\B", false),
        ],
        &[r"C:\Users\l\Desktop\Deal"],
    );
    assert_only_the_matters(
        &deny,
        true,
        &[
            r"C:\Users\l\Documents",
            r"C:\Users\l\Documents\Clients",
            r"C:\Users\l\DOCUME~1",
            r"C:\Users\l",
            r"C:\Users",
        ],
        &[r"C:\Users\l\Documents\Clients\B\f.txt"],
    );
}
