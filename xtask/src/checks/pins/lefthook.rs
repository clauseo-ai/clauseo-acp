//! The `lefthook.yml` rule: a comment that names a lefthook version names the one `BINARIES` in
//! `xtask/src/setup.rs` pins.
//!
//! YAML parsers drop comments, so this is the one place `pins` reads text outside a parser: it
//! parses the file, finds each comment with `yaml::Doc::comments` (a `#` outside every scalar's
//! span), and reads the word after `lefthook` in it.

use super::{Fix, LEFTHOOK, SETUP, yaml};
use crate::checks::{self, Inspection};
use crate::setup::Binary;
use std::path::Path;

/// The name of lefthook's entry in `BINARIES`.
const NAME: &str = "lefthook";

pub(super) fn check(root: &Path, binaries: &[Binary], found: &mut Inspection) {
    let text = match checks::read_repo_file(root, LEFTHOOK) {
        Ok(None) => return,
        Ok(Some(text)) => text,
        Err(unreadable) => {
            found.inspected = found.inspected.saturating_add(1);
            found.violations.push(unreadable);
            return;
        }
    };
    found.inspected = found.inspected.saturating_add(1);
    let doc = match yaml::parse(&text) {
        Ok(doc) => doc,
        Err(error) => {
            found.violations.push(checks::violation(
                LEFTHOOK,
                error.line,
                format!("{LEFTHOOK} is not YAML: {}", error.info),
                Fix::NotYaml.to_string(),
            ));
            return;
        }
    };
    let Some(pinned) = binaries.iter().find(|binary| binary.name == NAME) else {
        return;
    };
    for (line, comment) in doc.comments(&text) {
        for named in versions_named(comment) {
            if named != pinned.version {
                found.violations.push(checks::violation(
                    LEFTHOOK,
                    line,
                    format!(
                        "the comment names lefthook {named}, not {}, the version `BINARIES` in \
                         {SETUP} pins",
                        pinned.version
                    ),
                    Fix::LefthookComment {
                        pinned: pinned.version.to_owned(),
                    }
                    .to_string(),
                ));
            }
        }
    }
}

/// Each version `comment` names: the word after `lefthook` (any case) and a space or `@`, when
/// it starts with a digit, or with `v` and a digit. The `v` and any punctuation after the
/// version are dropped: `lefthook v1.2.3:` names `1.2.3`.
pub(super) fn versions_named(comment: &str) -> Vec<&str> {
    let lower = comment.to_ascii_lowercase();
    let mut out = Vec::new();
    for (at, _) in lower.match_indices(NAME) {
        let before = lower.get(..at).and_then(|text| text.chars().next_back());
        if before.is_some_and(|ch| ch.is_alphanumeric() || ch == '-' || ch == '_') {
            continue;
        }
        let rest = comment
            .get(at.saturating_add(NAME.len())..)
            .unwrap_or_default();
        let Some(rest) = rest
            .strip_prefix('@')
            .or_else(|| rest.starts_with([' ', '\t']).then(|| rest.trim_start()))
        else {
            continue;
        };
        let word = rest.split_whitespace().next().unwrap_or_default();
        let word = word.strip_prefix(['v', 'V']).unwrap_or(word);
        let version = word.trim_end_matches(|ch: char| !ch.is_ascii_alphanumeric());
        if version.starts_with(|ch: char| ch.is_ascii_digit()) {
            out.push(version);
        }
    }
    out
}
