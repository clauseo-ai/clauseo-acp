//! Windows: Claude's `Bash(…)` and `PowerShell(…)` deny rules for a command that names another
//! matter's folder (`agent::matter_fence`): each fence path of each other matter, the invited
//! ones too (an invite is read-only, and a shell can't be split into reads and writes, so on
//! Windows Claude reads an invited matter through its file tools only). One list per matter, so
//! the launch can keep as many matters as fit in the settings file and name the rest to the
//! lawyer (`agent::loads::profile`).
//!
//! Each fence path is written each way a command may write it ([`forms`]: `\`, `/`, `\\`, Git
//! Bash's `/c/…` and under the variables of `forms::VARS`), followed by anything: one rule per
//! form and shell. Where the session's own matter folder or `<data>` starts with the path's
//! text and goes on past it inside a name (a matter `Smith` beside the session's `Smith 2`),
//! the rules end at a name boundary instead ([`named`]): the path followed by a separator, one
//! of [`AFTER`](super::AFTER), or the end. Otherwise a name that merely starts with the path's
//! (another folder `Smith Jones`) is denied too: the accepted cost of a short list. The rest
//! of the residual is the data folder's (`super`'s module doc): a path built at run time, a `cd`
//! first, other letter cases in Bash, a short name, a script that opens the file.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;

use clauseo_protocol::MatterId;

use super::{Piece, forms, join, named, render, text};
use crate::agent::data_fence::LaunchDeny;

/// The rules for each other matter of `deny.matters` that has a fence path, in its order, with
/// the variable forms when `variables`; `var` reads the variables.
pub(crate) fn windows_matter_shell_rules(
    deny: &LaunchDeny,
    var: &dyn Fn(&str) -> Option<OsString>,
    variables: bool,
) -> Vec<(MatterId, Vec<String>)> {
    let kept: Vec<String> = deny
        .matters
        .own
        .iter()
        .chain(&deny.spellings)
        .filter_map(|place| back_form(place))
        .collect();
    let mut out = Vec::new();
    for other in &deny.matters.others {
        let mut tagged: Vec<(Vec<Piece>, bool, forms::Shells)> = Vec::new();
        for path in &other.paths {
            let Some(back) = back_form(path) else {
                continue;
            };
            let bounded = kept.iter().any(|place| {
                place
                    .strip_prefix(&back)
                    .is_some_and(|rest| !rest.is_empty() && !rest.starts_with('\\'))
            });
            let one = LaunchDeny {
                spellings: vec![path.clone()],
                ..LaunchDeny::default()
            };
            for form in forms(&one, var, variables) {
                let (mut contents, mut ends) = (Vec::new(), Vec::new());
                named(&text(&form.lead), bounded, &mut contents, &mut ends);
                tagged.extend(
                    contents
                        .into_iter()
                        .map(|pieces| (pieces, false, form.shells)),
                );
                tagged.extend(ends.into_iter().map(|pieces| (pieces, true, form.shells)));
            }
        }
        if !tagged.is_empty() {
            out.push((other.id, rendered(&tagged)));
        }
    }
    out
}

/// `tagged` as rules, each once per shell; PowerShell's compared in any case, as it matches them.
fn rendered(tagged: &[(Vec<Piece>, bool, forms::Shells)]) -> Vec<String> {
    let mut rules = Vec::new();
    for tool in ["Bash", "PowerShell"] {
        let mut seen = HashSet::new();
        for (pieces, end, shells) in tagged {
            if !shells.has(tool) {
                continue;
            }
            let content = render(&join(&[&[Piece::Any], pieces]), *end);
            let key = if tool == "PowerShell" {
                content.to_lowercase()
            } else {
                content.clone()
            };
            if seen.insert(key) {
                rules.push(format!("{tool}({content})"));
            }
        }
    }
    rules
}

/// `path` with `\` separators and no trailing one, folded to lower case, for the boundary test;
/// `None` when it is not UTF-8.
fn back_form(path: &Path) -> Option<String> {
    let text = path.to_str()?.replace('/', "\\");
    Some(text.trim_end_matches('\\').to_lowercase())
}

#[cfg(test)]
#[path = "shell_deny_matters_tests.rs"]
mod tests;
