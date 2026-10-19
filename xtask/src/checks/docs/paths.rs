//! Backticked repo paths: a code span that starts with one of the repo's top-level folders must
//! name a file or folder git lists (see `Tree`), with the same case. `crates/<row>` is exempt
//! while the Crates-table row is planned. A span that is a pattern or a placeholder rather than a
//! path (it holds `<`, `>`, `*`, `?`, `{` or `...`) is not read, a trailing `:<line>` or
//! `:<line>:<col>` is cut before the lookup, and a path with a `..` part fails.

use super::markdown::Span;
use super::tree::Tree;
use crate::checks::violation;
use crate::violation::Violation;

/// The starts that make a code span a repo path.
const FOLDERS: [&str; 6] = ["crates/", "ui/", "xtask/", "docs/", "LICENSES/", ".github/"];

/// What in a span makes it a pattern or a placeholder (`crates/<name>/`, `xtask/**`,
/// `docs/*.md`, `ui/{app,components}`, `crates/...`), not a path to look up.
const PATTERN: [&str; 6] = ["<", ">", "*", "?", "{", "..."];

/// One violation per repo path in `spans` that does not exist or climbs with `..`, at the span's
/// line. `planned` holds the names of the Crates-table rows whose status is `planned`.
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
    if !is_repo_path(text) || PATTERN.iter().any(|mark| text.contains(mark)) {
        return None;
    }
    let path = without_location(text);
    if path.split('/').any(|part| part == "..") {
        return Some(violation(
            file,
            span.line,
            format!("`{path}` has a `..` part"),
            "write the path from the repo root, without `..`",
        ));
    }
    if is_planned(path, planned) || tree.contains(&normalized(path)) {
        return None;
    }
    Some(violation(
        file,
        span.line,
        format!("`{path}` does not exist"),
        format!("create `{path}`, or correct or remove the reference"),
    ))
}

/// True when the code span's content is a path under one of `FOLDERS`.
fn is_repo_path(text: &str) -> bool {
    FOLDERS.iter().any(|folder| text.starts_with(folder))
}

/// `path` without a trailing `:<line>` or `:<line>:<col>`.
fn without_location(path: &str) -> &str {
    match without_number(path) {
        Some(head) => without_number(head).unwrap_or(head),
        None => path,
    }
}

/// `text` without a trailing `:<digits>`, or `None` when it has none.
fn without_number(text: &str) -> Option<&str> {
    let (head, number) = text.rsplit_once(':')?;
    (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some(head)
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
    }

    #[test]
    fn a_trailing_slash_and_dot_parts_are_dropped_for_the_lookup() {
        assert_eq!(normalized(".github/"), ".github");
        assert_eq!(normalized("docs/./a.md"), "docs/a.md");
    }
}
