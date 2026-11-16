//! The `rules` folder of a Codex agent's home, removed at every launch ([`remove_rules`]).

use std::path::Path;

use crate::error::Error;
use crate::log_file::Cause;

use super::home_failed;

/// The folder in a Codex agent's home that Codex loads rules from at every start, and appends
/// to when an approval amends its policy (codex rust-v0.162.1 `core/src/exec_policy.rs:54-56,
/// 464-560, 678-682`): a `prefix_rule` there runs matching commands unasked and outside the
/// sandbox, in every session and matter of the agent.
pub(crate) const RULES_FOLDER: &str = "rules";

/// Removes `<home>/rules` before Codex starts, at every launch: no card can make a rule any
/// more (`agent/codex_options.rs`), and this drops those an earlier build's "Always" left,
/// from this launch on. Whatever is there goes: a folder with all it holds, a plain file of
/// that name, and a link, which is removed and never followed (`symlink_metadata`;
/// `remove_dir_all` does not follow links inside either). Nothing there is fine. A removal
/// that fails refuses the launch, as a home that cannot be made does ([`home_failed`]): Codex
/// would otherwise start with the rules. `tracing` gets a warning with the entries' names
/// (never their contents, never the path) and how many `.rules` files went.
pub(crate) fn remove_rules(home: &Path) -> Result<(), Error> {
    let rules = home.join(RULES_FOLDER);
    let meta = match std::fs::symlink_metadata(&rules) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(rules_failed(&error)),
        Ok(meta) => meta,
    };
    let kind = meta.file_type();
    // A link (a junction too, on Windows) is never taken for a folder, whatever it points to.
    let folder = kind.is_dir() && !kind.is_symlink();
    let (names, rule_files) = if folder {
        rules_entries(&rules)
    } else {
        (Vec::new(), 0)
    };
    let shape = if kind.is_symlink() {
        "link"
    } else if folder {
        "folder"
    } else {
        "file"
    };
    let removed = if folder {
        crate::write::remove_tree(&rules)
    } else {
        crate::write::remove_link(&rules)
    };
    // Gone already (another launch of the agent removed it): nothing is left to load.
    let removed = match removed {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        other => other,
    };
    removed.map_err(|error| rules_failed(&error))?;
    tracing::warn!(
        shape,
        rule_files,
        ?names,
        "removed the rules an earlier approval left in Codex's home"
    );
    Ok(())
}

/// The names of the entries in the rules folder, sorted (the first [`LOGGED_NAMES`]), and how
/// many are `.rules` files (as Codex loads them). A listing that fails gives what it read;
/// the removal still runs.
fn rules_entries(rules: &Path) -> (Vec<String>, usize) {
    let mut names: Vec<String> = std::fs::read_dir(rules)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let rule_files = names
        .iter()
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("rules"))
        })
        .count();
    names.truncate(LOGGED_NAMES);
    (names, rule_files)
}

/// How many of the rules folder's entry names the warning lists, at most.
const LOGGED_NAMES: usize = 20;

/// The failure of a Codex launch whose `rules` could not be removed: Clauseo's plain words for
/// a home it could not prepare, the io error's kind in the log.
fn rules_failed(error: &std::io::Error) -> Error {
    tracing::warn!(error_kind = ?error.kind(), "the rules in Codex's home could not be removed");
    home_failed(&Cause(error))
}

#[cfg(test)]
mod tests;
