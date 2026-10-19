//! Backticked repo paths: a code span whose first word (up to whitespace) starts with one of the
//! repo's top-level folders (in any case) must name a file or folder git lists (see `Tree`), with
//! the same case. The words after it are not read: `crates/host/src/main.rs --help` checks the
//! file. A first word that is a pattern or a placeholder (it holds `<`, `>`, `*`, `?`, `{` or
//! `...`) is not read. An anchor (`#sec`) and trailing `,;:` are cut, and then a path with a
//! `..` part fails, so a sentence's period cannot turn `crates/..` into `crates/.`. Before the
//! lookup one trailing `.` is cut, then trailing `,;:` again, then a trailing `:<line>`.
//!
//! A path that does not exist yet is marked: `crates/<row>` while its Crates-table row is
//! planned, and, in `ARCHITECTURE.md` only (the file that says what is planned), a path written
//! `` `path` (planned) ``. In any other file the marker is not read and the path must exist. The
//! marker holds both ways, as the table's Status does: a marked path that exists fails, so the
//! marker is dropped when the path lands, and so does one that differs from a listed path, or
//! whose folder differs from a listed folder, only in case. The cost: a marked path with a typo
//! passes until a file by that name appears.
//!
//! A missing path that is on disk but not listed fails with a message that says git ignores it.

use super::markdown::{PLANNED, Span};
use super::tree::{self, Tree};
use crate::checks::violation;
use crate::violation::Violation;
use std::path::Path;

/// The starts that make a code span a repo path.
const FOLDERS: [&str; 6] = ["crates/", "ui/", "xtask/", "docs/", "LICENSES/", ".github/"];

/// What in a span's first word makes it a pattern or a placeholder (`crates/<name>/`, `xtask/**`,
/// `docs/*.md`, `ui/{app,components}`, `crates/...`), not a path to look up.
const PATTERN: [&str; 6] = ["<", ">", "*", "?", "{", "..."];

/// Punctuation that ends a clause or a list after a path, not part of it. A sentence's `.` is
/// cut separately, once, after the `..` check.
const TRAILING: [char; 3] = [',', ';', ':'];

/// The one file whose ` (planned)` markers are read.
const PLAN_FILE: &str = "ARCHITECTURE.md";

/// One violation per repo path in `spans` of `file` that does not exist, climbs with `..`, or
/// is marked planned but exists in some case, at the span's line. `root` is the repo on disk,
/// read only to word the message for a path git does not list. `planned` holds the names of the
/// Crates-table rows whose status is `planned`.
pub(super) fn problems(
    tree: &Tree,
    root: &Path,
    file: &str,
    spans: &[Span],
    planned: &[String],
) -> Vec<Violation> {
    spans
        .iter()
        .filter_map(|span| problem(tree, root, file, span, planned))
        .collect()
}

/// The violation for `span` when its first word is a repo path that is not in `tree`.
fn problem(
    tree: &Tree,
    root: &Path,
    file: &str,
    span: &Span,
    planned: &[String],
) -> Option<Violation> {
    let text = path_word(&span.text)?;
    let cut = without_anchor(text);
    if cut.split('/').any(|part| part == "..") {
        return Some(violation(
            file,
            span.line,
            format!("`{cut}` has a `..` part"),
            "write the path from the repo root, without `..`",
        ));
    }
    let path = bare(cut);
    let key = normalized(path);
    let exists = tree.contains(&key);
    if span.planned && file == PLAN_FILE {
        return if exists {
            Some(violation(
                file,
                span.line,
                format!("`{path}` exists, but the text marks it{PLANNED}"),
                format!("drop the{PLANNED} after `{path}`"),
            ))
        } else {
            case_problem(tree, file, span.line, path, &key)
        };
    }
    if exists || is_planned(path, planned) {
        return None;
    }
    if let Some(found) = tree::on_disk(root, &key) {
        return Some(violation(
            file,
            span.line,
            found.reason(path),
            "name a file git tracks, or remove the reference",
        ));
    }
    Some(violation(
        file,
        span.line,
        format!("`{path}` does not exist"),
        format!("create `{path}`, or correct or remove the reference"),
    ))
}

/// The violation for `path` (`key` as the tree holds it), marked planned and not listed, when
/// it, or failing that one of the folders it starts with, is listed in another case.
fn case_problem(tree: &Tree, file: &str, line: u32, path: &str, key: &str) -> Option<Violation> {
    if let Some(real) = tree.ignoring_case(key) {
        return Some(violation(
            file,
            line,
            format!("`{path}` is marked{PLANNED}, but `{real}` exists, in another case"),
            format!("write `{real}`, and drop the{PLANNED} after it"),
        ));
    }
    let folders = key.match_indices('/').filter_map(|(at, _)| key.get(..at));
    let (folder, real) = folders
        .filter(|folder| !tree.contains(folder))
        .find_map(|folder| tree.ignoring_case(folder).map(|real| (folder, real)))?;
    Some(violation(
        file,
        line,
        format!("`{path}` is marked{PLANNED}, but its folder `{folder}` exists as `{real}`"),
        format!("write the path under `{real}`, as git lists it"),
    ))
}

/// The first word of `span` (up to whitespace) when it is a repo path to look up: it starts with
/// one of `FOLDERS` and holds none of `PATTERN`.
fn path_word(span: &str) -> Option<&str> {
    let word = span.split_whitespace().next()?;
    let is_pattern = PATTERN.iter().any(|mark| word.contains(mark));
    (is_repo_path(word) && !is_pattern).then_some(word)
}

/// True when the code span's content starts with one of `FOLDERS`, in any case: `Docs/x.md` is
/// read, and then fails the exact lookup.
fn is_repo_path(text: &str) -> bool {
    FOLDERS.iter().any(|folder| {
        text.get(..folder.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(folder))
    })
}

/// `text` without an anchor and trailing `,;:`; dots are left for the `..` check.
fn without_anchor(text: &str) -> &str {
    let path = text.split('#').next().unwrap_or_default();
    path.trim_end_matches(TRAILING)
}

/// The path `cut` (see `without_anchor`) names: one sentence-ending `.` cut, then trailing `,;:`,
/// then a line location.
fn bare(cut: &str) -> &str {
    let path = cut.strip_suffix('.').unwrap_or(cut);
    without_location(path.trim_end_matches(TRAILING))
}

/// `path` without a trailing `:<line>`. No doc writes `:<line>:<col>` or a line range, so those
/// stay and fail the lookup.
fn without_location(path: &str) -> &str {
    match path.rsplit_once(':') {
        Some((head, line)) if !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()) => head,
        _ => path,
    }
}

/// `path` as the tree holds it: no empty or `.` parts, so no trailing `/`.
fn normalized(path: &str) -> String {
    path.split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// True for `crates/<name>` or `crates/<name>/...` where `<name>` is a planned row.
fn is_planned(path: &str, planned: &[String]) -> bool {
    let mut parts = path.split('/');
    parts.next() == Some("crates")
        && parts
            .next()
            .is_some_and(|name| planned.iter().any(|row| row == name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_is_cut_and_other_colons_stay() {
        assert_eq!(
            without_location("crates/a/src/main.rs:12"),
            "crates/a/src/main.rs"
        );
        assert_eq!(
            without_location("crates/a/src/main.rs:12:5"),
            "crates/a/src/main.rs:12"
        );
        assert_eq!(
            without_location("crates/a/src/main.rs"),
            "crates/a/src/main.rs"
        );
        assert_eq!(without_location("docs/a:b.md"), "docs/a:b.md");
        assert_eq!(without_location("docs/a.md:"), "docs/a.md:");
        assert_eq!(without_location("docs/a.md:1-3"), "docs/a.md:1-3");
        assert_eq!(without_location("docs/a.md:1-"), "docs/a.md:1-");
    }

    #[test]
    fn an_anchor_trailing_punctuation_and_a_location_are_cut_in_that_order() {
        let path = |text| bare(without_anchor(text));
        assert_eq!(path("docs/a.md#sec"), "docs/a.md");
        assert_eq!(path("docs/a.md,"), "docs/a.md");
        assert_eq!(path("docs/a.md:12."), "docs/a.md");
        assert_eq!(path("docs/a.md#L3:"), "docs/a.md");
        assert_eq!(path("docs/a.md.,"), "docs/a.md");
        assert_eq!(path(".github/"), ".github/");
    }

    #[test]
    fn dots_stay_for_the_dot_dot_check_and_only_one_is_cut_after_it() {
        assert_eq!(without_anchor("crates/a/..,"), "crates/a/..");
        assert_eq!(without_anchor("crates/.."), "crates/..");
        assert_eq!(bare("crates/a/x.."), "crates/a/x.");
    }

    #[test]
    fn the_first_word_is_the_path_unless_it_is_a_pattern() {
        assert_eq!(
            path_word("crates/host/src/main.rs --help"),
            Some("crates/host/src/main.rs")
        );
        assert_eq!(path_word("crates/nope.rs --flag"), Some("crates/nope.rs"));
        assert_eq!(path_word("crates/<name>/ and more"), None);
        assert_eq!(path_word("docs/*.md and more"), None);
        assert_eq!(path_word("see crates/a.rs"), None);
        assert_eq!(path_word(""), None);
    }

    #[test]
    fn the_top_folder_is_matched_in_any_case() {
        assert!(is_repo_path("Docs/a.md"));
        assert!(is_repo_path("XTASK/src"));
        assert!(!is_repo_path("doc/a.md"));
        assert!(!is_repo_path("ui"));
    }

    #[test]
    fn a_trailing_slash_and_dot_parts_are_dropped_for_the_lookup() {
        assert_eq!(normalized(".github/"), ".github");
        assert_eq!(normalized("docs/./a.md"), "docs/a.md");
    }
}
