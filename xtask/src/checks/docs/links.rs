//! Relative links: each link destination that is not an absolute URL, a pure `#anchor` or a link
//! into the `clauseo-acp-private` folder beside the repo must resolve to a file or folder in the
//! repo: a file or folder git lists (see `Tree`), named with the same case. `..` is resolved
//! against the linking file's folder, and a link that climbs above the root fails. A target that
//! is on disk but not listed fails with a message that says git ignores it.
//!
//! A label defined twice (`[e]: a.md` and later `[e]: nope.md`) is read once: pulldown-cmark's
//! `reference_definitions()` keeps only the first definition, so a broken repeat is never read.

use super::markdown::Link;
use super::tree::{self, Tree};
use crate::checks::violation;
use crate::violation::Violation;
use std::path::Path;

/// The folder beside the repo that may be absent. Links into it are not checked.
const PRIVATE_FOLDER: &str = "clauseo-acp-private";

/// The broken relative links of `file` (repo-relative), one violation each, at the link's line.
/// `root` is the repo on disk, read only to word the message for a target git does not list.
pub(super) fn problems(tree: &Tree, root: &Path, file: &str, links: &[Link]) -> Vec<Violation> {
    links
        .iter()
        .filter_map(|link| problem(tree, root, file, link))
        .collect()
}

/// The violation for `link` when it is a relative link that does not resolve inside the repo.
fn problem(tree: &Tree, root: &Path, file: &str, link: &Link) -> Option<Violation> {
    let path = relative_path(&link.target)?;
    match resolve(dir_of(file), &percent_decode(path)) {
        Target::Inside(rel) if tree.contains(&rel) => None,
        Target::Inside(rel) => match tree::on_disk(root, &rel) {
            Some(found) => Some(violation(
                file,
                link.line,
                format!(
                    "the relative link `{path}` does not resolve: {}",
                    found.reason(&rel)
                ),
                "link a file git tracks, or remove the link",
            )),
            None => Some(violation(
                file,
                link.line,
                format!("the relative link `{path}` does not resolve: `{rel}` does not exist"),
                format!("create `{rel}`, or correct or remove the link"),
            )),
        },
        Target::Outside { private: true } => None,
        Target::Outside { private: false } => Some(violation(
            file,
            link.line,
            format!("the relative link `{path}` leaves the repo"),
            "point it at a file inside the repo, or remove it",
        )),
    }
}

/// The path part of a link destination, cut at the first `?` or `#` (a query such as `?plain=1`,
/// or an anchor), or `None` for a destination this check does not resolve: an absolute URL (any
/// scheme, or `//host`), a pure `#anchor`, or an empty path.
fn relative_path(target: &str) -> Option<&str> {
    if target.starts_with('#') || target.starts_with("//") || has_scheme(target) {
        return None;
    }
    let path = target.split(['?', '#']).next().unwrap_or_default();
    (!path.is_empty()).then_some(path)
}

/// True when `target` starts with a URL scheme such as `https:` or `mailto:`.
fn has_scheme(target: &str) -> bool {
    target.split_once(':').is_some_and(|(scheme, _)| {
        scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    })
}

/// The folder of `file` (repo-relative), `""` for a file at the root.
fn dir_of(file: &str) -> &str {
    file.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// Where a link lands.
#[derive(Debug, PartialEq, Eq)]
enum Target {
    /// Inside the repo, at this `/`-separated path (`""` is the root).
    Inside(String),
    /// Above the repo root. `private` when it is the `clauseo-acp-private` folder beside it.
    Outside { private: bool },
}

/// Resolves `target` against the folder `dir` of the linking file. A leading `/` starts at the
/// repo root instead, as Markdown renderers read it.
fn resolve(dir: &str, target: &str) -> Target {
    let base = if target.starts_with('/') { "" } else { dir };
    let mut inside: Vec<&str> = Vec::new();
    let mut up = 0usize;
    let mut below: Vec<&str> = Vec::new();
    for part in base.split('/').chain(target.split('/')) {
        match part {
            "" | "." => {}
            ".." if up == 0 => {
                if inside.pop().is_none() {
                    up = 1;
                }
            }
            ".." => {
                if below.pop().is_none() {
                    up += 1;
                }
            }
            name if up == 0 => inside.push(name),
            name => below.push(name),
        }
    }
    if up == 0 {
        Target::Inside(inside.join("/"))
    } else {
        Target::Outside {
            private: up == 1 && below.first() == Some(&PRIVATE_FOLDER),
        }
    }
}

/// `path` with each `%XX` read as the byte it names. A `%` not followed by two hex digits stays.
fn percent_decode(path: &str) -> String {
    let bytes = path.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        let escaped = bytes
            .get(i + 1..i + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .filter(|pair| pair.bytes().all(|b| b.is_ascii_hexdigit()))
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        if let (b'%', Some(decoded)) = (byte, escaped) {
            out.push(decoded);
            i += 3;
        } else {
            out.push(byte);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests;
