//! Backticked repo paths: a code span that starts with one of the repo's top-level folders must
//! name an existing file or folder. `crates/<row>` is exempt while the Crates-table row is planned.

use super::markdown::Span;
use crate::checks::violation;
use crate::violation::Violation;
use std::path::Path;

/// The starts that make a code span a repo path.
const FOLDERS: [&str; 6] = ["crates/", "ui/", "xtask/", "docs/", "LICENSES/", ".github/"];

/// One violation per repo path in `spans` that does not exist, at the span's line. `planned` holds
/// the names of the Crates-table rows whose status is `planned`.
pub(super) fn problems(
    root: &Path,
    file: &str,
    spans: &[Span],
    planned: &[String],
) -> Vec<Violation> {
    spans
        .iter()
        .filter(|span| is_repo_path(&span.text) && !is_planned(&span.text, planned))
        .filter(|span| !root.join(&span.text).exists())
        .map(|span| {
            violation(
                file,
                span.line,
                format!("`{}` does not exist", span.text),
                format!("create `{}`, or correct or remove the reference", span.text),
            )
        })
        .collect()
}

/// True when the code span's content is a path under one of `FOLDERS`.
fn is_repo_path(text: &str) -> bool {
    FOLDERS.iter().any(|folder| text.starts_with(folder))
}

/// True for `crates/<name>` or `crates/<name>/...` where `<name>` is a planned row.
fn is_planned(path: &str, planned: &[String]) -> bool {
    let mut parts = path.split('/');
    parts.next() == Some("crates")
        && parts
            .next()
            .is_some_and(|name| planned.iter().any(|row| row == name))
}
