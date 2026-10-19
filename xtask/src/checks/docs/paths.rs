//! Backticked repo paths: a code span that starts with one of the repo's top-level folders (in any
//! case) must name a file or folder git lists (see `Tree`), with the same case. A span that is a
//! pattern, a placeholder or prose rather than a path (it holds `<`, `>`, `*`, `?`, `{`, `...` or
//! whitespace) is not read. Before the lookup an anchor (`#sec`) is cut, then trailing `,.;:`,
//! then a trailing `:<line>`, `:<line>:<col>` or `:<line>-<line>`. A path with a `..` part fails.
//!
//! A path that does not exist yet is marked: `crates/<row>` while its Crates-table row is
//! planned, and any path written `` `path` (planned) ``. The marker holds both ways, as the
//! table's Status does: a marked path that exists fails, so the marker is dropped when the path
//! lands.

use super::markdown::{PLANNED, Span};
use super::tree::Tree;
use crate::checks::violation;
use crate::violation::Violation;

/// The starts that make a code span a repo path.
const FOLDERS: [&str; 6] = ["crates/", "ui/", "xtask/", "docs/", "LICENSES/", ".github/"];

/// What in a span makes it a pattern or a placeholder (`crates/<name>/`, `xtask/**`,
/// `docs/*.md`, `ui/{app,components}`, `crates/...`), not a path to look up.
const PATTERN: [&str; 6] = ["<", ">", "*", "?", "{", "..."];

/// Punctuation that ends a sentence or a list after a path, not part of it.
const TRAILING: [char; 4] = [',', '.', ';', ':'];

/// One violation per repo path in `spans` that does not exist, climbs with `..`, or is marked
/// planned but exists, at the span's line. `planned` holds the names of the Crates-table rows
/// whose status is `planned`.
pub(super) fn problems(
    tree: &Tree,
    file: &str,
    spans: &[Span],
    planned: &[String],
) -> Vec<Violation> {
    spans
        .iter()
        .filter_map(|span| problem(tree, file, span, planned))
        .collect()
}

/// The violation for `span` when it is a repo path that is not in `tree`.
fn problem(tree: &Tree, file: &str, span: &Span, planned: &[String]) -> Option<Violation> {
    let text = span.text.as_str();
    let is_pattern = PATTERN.iter().any(|mark| text.contains(mark));
    if !is_repo_path(text) || is_pattern || text.contains(char::is_whitespace) {
        return None;
    }
    let path = bare(text);
    if path.split('/').any(|part| part == "..") {
        return Some(violation(
            file,
            span.line,
            format!("`{path}` has a `..` part"),
            "write the path from the repo root, without `..`",
        ));
    }
    let exists = tree.contains(&normalized(path));
    if span.planned {
        return exists.then(|| {
            violation(
                file,
                span.line,
                format!("`{path}` exists, but the text marks it{PLANNED}"),
                format!("drop the{PLANNED} after `{path}`"),
            )
        });
    }
    if exists || is_planned(path, planned) {
        return None;
    }
    Some(violation(
        file,
        span.line,
        format!("`{path}` does not exist"),
        format!("create `{path}`, or correct or remove the reference"),
    ))
}

/// True when the code span's content starts with one of `FOLDERS`, in any case: `Docs/x.md` is
/// read, and then fails the exact lookup.
fn is_repo_path(text: &str) -> bool {
    FOLDERS.iter().any(|folder| {
        text.get(..folder.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(folder))
    })
}

/// The path a span names: anchor cut, then trailing punctuation, then a line location.
fn bare(text: &str) -> &str {
    let path = text.split('#').next().unwrap_or_default();
    without_location(path.trim_end_matches(TRAILING))
}

/// `path` without a trailing `:<line>`, `:<line>:<col>` or `:<line>-<line>`.
fn without_location(path: &str) -> &str {
    match without_number(path) {
        Some(head) => without_number(head).unwrap_or(head),
        None => path,
    }
}

/// `text` without a trailing `:<digits>` or `:<digits>-<digits>`, or `None` when it has none.
fn without_number(text: &str) -> Option<&str> {
    let (head, number) = text.rsplit_once(':')?;
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    let is_location = match number.split_once('-') {
        Some((first, last)) => digits(first) && digits(last),
        None => digits(number),
    };
    is_location.then_some(head)
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
    fn a_line_and_a_column_are_cut_and_other_colons_stay() {
        assert_eq!(
            without_location("crates/a/src/main.rs:12"),
            "crates/a/src/main.rs"
        );
        assert_eq!(
            without_location("crates/a/src/main.rs:12:5"),
            "crates/a/src/main.rs"
        );
        assert_eq!(
            without_location("crates/a/src/main.rs"),
            "crates/a/src/main.rs"
        );
        assert_eq!(without_location("docs/a:b.md"), "docs/a:b.md");
        assert_eq!(without_location("docs/a.md:"), "docs/a.md:");
        assert_eq!(without_location("docs/a.md:1-3"), "docs/a.md");
        assert_eq!(without_location("docs/a.md:1-"), "docs/a.md:1-");
    }

    #[test]
    fn an_anchor_trailing_punctuation_and_a_location_are_cut_in_that_order() {
        assert_eq!(bare("docs/a.md#sec"), "docs/a.md");
        assert_eq!(bare("docs/a.md,"), "docs/a.md");
        assert_eq!(bare("docs/a.md:12."), "docs/a.md");
        assert_eq!(bare("docs/a.md#L3:"), "docs/a.md");
        assert_eq!(bare(".github/"), ".github/");
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
