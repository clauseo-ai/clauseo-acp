//! The exact `Edit` denies of the folders above other matters (`super::claude_deny_rules`).

use super::{LaunchDeny, rule_path};
use crate::network_paths::fold_name;

/// `Edit` denies of the folders above each fence path of every other matter, invited or not,
/// that hold neither the session's own matter (`MatterFence::own`, each spelling) nor the data
/// folder (each spelling), in each spelling the fence has, once each; never a drive root. Claude
/// Code checks the paths of a command that moves, renames or removes (`mv`, `rm`, `Move-Item`,
/// `Rename-Item`, `Remove-Item`) against its `Edit` denies, so moving such a folder, which would
/// carry the other matter with it, is refused. Claude Code reads a rule as gitignore does, so
/// `Edit(<folder>)` covers all inside it too: hence never a folder above the own matter or the
/// data folder (its own run folder), and none at all before the launch resolved the own matter.
/// What is left: a move of a folder that holds both the own matter and another (the matters'
/// shared parent) is in no rule. Whether Claude Code's matcher refuses `Move-Item <folder> …`
/// on such a rule is pending its confirmation.
pub(super) fn ancestor_edit_rules(deny: &LaunchDeny, windows: bool) -> Vec<String> {
    let folded = |rule: &str| {
        if windows || crate::network_paths::FOLD_CASE {
            fold_name(rule)
        } else {
            rule.to_owned()
        }
    };
    let kept: Vec<String> = deny
        .matters
        .own
        .iter()
        .chain(&deny.spellings)
        .filter_map(|place| place.to_str())
        .map(|text| folded(rule_path(text, windows).trim_end_matches('/')))
        .collect();
    if deny.matters.own.is_empty() {
        return Vec::new();
    }
    // Whether the folder `at` (folded) is a kept place or above one.
    let holds_kept = |at: &str| {
        kept.iter().any(|place| {
            place == at
                || place
                    .strip_prefix(at)
                    .is_some_and(|rest| rest.starts_with('/'))
        })
    };
    let mut rules = Vec::new();
    for path in deny.matters.denied_edits() {
        let Some(base) = path.to_str().map(|text| rule_path(text, windows)) else {
            continue;
        };
        let mut at = base.trim_end_matches('/');
        // `//c/Clients/Q/C` gives `//c/Clients/Q`, then `//c/Clients`; never `//c` (a drive
        // root, or a share's server) nor `//`.
        while let Some((above, _)) = at.rsplit_once('/') {
            let mut names = above.trim_start_matches('/').split('/');
            let drive = names.next().is_some_and(|first| first.len() == 1);
            // A drive root is one name (`//c`), a share's two (`//server/share`).
            let root = if drive { 1 } else { 2 };
            if above.len() <= 2 || (windows && names.count() < root) {
                break;
            }
            at = above;
            if holds_kept(&folded(at)) {
                // Every folder above it holds the kept place too.
                break;
            }
            let rule = format!("Edit({at})");
            if !rules.contains(&rule) {
                rules.push(rule);
            }
        }
    }
    rules
}
