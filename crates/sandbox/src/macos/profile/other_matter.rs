//! Reads of other matters (R18): each grant read again after the hidden paths, and the names
//! that stay refused in it denied after that, at any depth and in any letter case.

use std::path::PathBuf;

use super::{Writer, ancestors, literal, subpath};
use crate::{Policy, REFUSED_IN_OTHER_MATTER};

/// One pattern on the path for every one of [`REFUSED_IN_OTHER_MATTER`] as a whole name, each
/// letter in both cases (Seatbelt's regex has no case flag): `/\.(git|clauseo|...)(/|$)`. The
/// policy refuses a grant with one of them anywhere in its own path, so within a grant the
/// pattern only matches names below it.
pub(super) fn refused_regex() -> String {
    let names: Vec<String> = REFUSED_IN_OTHER_MATTER
        .iter()
        .map(|name| {
            name.trim_start_matches('.')
                .chars()
                .map(|c| {
                    if c.is_ascii_alphabetic() {
                        format!("[{}{}]", c.to_ascii_lowercase(), c.to_ascii_uppercase())
                    } else {
                        c.to_string()
                    }
                })
                .collect()
        })
        .collect();
    format!("/\\.({})(/|$)", names.join("|"))
}

/// The reads of other matters, after the rules allowed again after the hidden paths: a
/// tree as a subpath, a file as a literal, read only; then the refused names in each tree
/// denied read and write. Seatbelt applies the last rule that matches, and checks the
/// path a link resolves to, so a link in the grant into its `.git` is refused too.
pub(super) fn other_matter_reads(w: &mut Writer, policy: &Policy) {
    let reads: Vec<(String, PathBuf, bool)> = policy
        .other_matter_reads()
        .iter()
        .enumerate()
        .map(|(i, read)| {
            let name = format!("OTHER_MATTER_READ_{i}");
            (name, read.path().to_owned(), read.is_file())
        })
        .collect();
    if reads.is_empty() {
        return;
    }
    w.note("\n; Reads of other matters (R18): read again after the hidden paths, never");
    w.note("; written.");
    let mut filters = Vec::new();
    let mut above = Vec::new();
    let mut trees = Vec::new();
    for (name, path, is_file) in &reads {
        let param = w.param(name, path);
        if *is_file {
            filters.push(literal(&param));
        } else {
            filters.push(subpath(&param));
            trees.push(param.clone());
        }
        above.push(ancestors(&param));
    }
    w.rule("allow", "file-read*", &filters);
    w.note("; The folders above them: metadata only, so their paths resolve.");
    w.rule("allow", "file-read-metadata", &above);
    w.note("; In them, at any depth and in any letter case: no .git, .clauseo, .claude,");
    w.note("; .codex or .agents.");
    let regex = refused_regex();
    let refused: Vec<String> = trees
        .iter()
        .map(|param| format!("(require-all {} (regex #\"{regex}\"))", subpath(param)))
        .collect();
    w.rule("deny", "file-read* file-write*", &refused);
}
