//! The marker line each probe edits: where it is, what it holds, the next value to write, and
//! whether a saved copy of the file may be put back.

use std::path::Path;

/// The start of the marker line. The probe writes a number after it, then `;`.
const PREFIX: &str = "pub const BUILDTIME_MARKER: u64 = ";

/// A crate's marker file: its repo-relative path, and its text as read before any probe ran.
pub(crate) struct Marker {
    pub path: String,
    pub text: String,
}

/// The repo-relative path of `file` in the crate at `dir`.
pub(crate) fn in_crate(dir: &str, file: &str) -> String {
    if dir.is_empty() {
        file.to_owned()
    } else {
        format!("{dir}/{file}")
    }
}

/// The first of `src/lib.rs` and `src/main.rs` in the crate at `dir` that holds the marker line.
pub(crate) fn find(root: &Path, dir: &str) -> Option<Marker> {
    ["src/lib.rs", "src/main.rs"].into_iter().find_map(|file| {
        let path = in_crate(dir, file);
        let text = std::fs::read_to_string(root.join(&path)).ok()?;
        bump_marker(&text).map(|_| Marker { path, text })
    })
}

/// `text` with the marker's number raised by one, every other byte unchanged. `None` when there is
/// no marker line, or its number does not fit.
pub(crate) fn bump_marker(text: &str) -> Option<String> {
    rewrite_first_marker(text, |indent, number, tail| {
        let next = number.parse::<u64>().ok()?.checked_add(1)?;
        Some(format!("{indent}{PREFIX}{next}{tail}"))
    })
}

/// `text` with the number of its first marker line removed. `None` when there is no marker line.
fn blank_marker_number(text: &str) -> Option<String> {
    rewrite_first_marker(text, |indent, _number, tail| {
        Some(format!("{indent}{PREFIX}{tail}"))
    })
}

/// `text` with its first marker line replaced by `rewrite(indent, number, tail)`. Every other line
/// is kept as it is. `None` when there is no marker line, or `rewrite` refuses it.
fn rewrite_first_marker(
    text: &str,
    rewrite: impl Fn(&str, &str, &str) -> Option<String>,
) -> Option<String> {
    let mut out = String::with_capacity(text.len() + 1);
    let mut found = false;
    for line in text.split_inclusive('\n') {
        match (found, marker_parts(line)) {
            (false, Some((indent, number, tail))) => {
                out.push_str(&rewrite(indent, number, tail)?);
                found = true;
            }
            _ => out.push_str(line),
        }
    }
    found.then_some(out)
}

/// The indent, the number and the tail (from `;` on) of a marker line, or `None` when the line is
/// not one.
fn marker_parts(line: &str) -> Option<(&str, &str, &str)> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let rest = body.strip_prefix(PREFIX)?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, tail) = rest.split_at(digits);
    if digits == 0 || !tail.starts_with(';') {
        return None;
    }
    Some((indent, number, tail))
}

/// What an interrupted run's saved copy means for the repo file as it is now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Leftover {
    /// The copy has no marker line, so it is not a saved marker file (a stray `.DS_Store`, say).
    NotACopy,
    /// The file equals the copy except for the marker's number: only the bump is undone.
    Restore,
    /// The file is gone.
    Missing,
    /// The file changed in some other way since the copy was saved, or lost its marker line.
    Conflict,
}

/// Whether the saved `copy` of a marker file may be put back over the file as it is now (`repo`,
/// `None` when the file is gone). Only the marker's number may differ.
pub(crate) fn leftover(repo: Option<&str>, copy: &str) -> Leftover {
    let Some(saved) = blank_marker_number(copy) else {
        return Leftover::NotACopy;
    };
    let Some(repo) = repo else {
        return Leftover::Missing;
    };
    match blank_marker_number(repo) {
        Some(now) if now == saved => Leftover::Restore,
        _ => Leftover::Conflict,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_marker_raises_the_number_by_one() {
        assert_eq!(
            bump_marker("pub const BUILDTIME_MARKER: u64 = 41;\n").as_deref(),
            Some("pub const BUILDTIME_MARKER: u64 = 42;\n")
        );
    }

    #[test]
    fn bump_marker_keeps_the_other_lines_and_the_indent() {
        let text = "//! crate\n\n    pub const BUILDTIME_MARKER: u64 = 0;\r\nfn f() {}";
        assert_eq!(
            bump_marker(text).as_deref(),
            Some("//! crate\n\n    pub const BUILDTIME_MARKER: u64 = 1;\r\nfn f() {}")
        );
    }

    #[test]
    fn bump_marker_is_none_without_a_marker_line() {
        assert_eq!(bump_marker("pub const OTHER: u64 = 1;\n"), None);
        assert_eq!(bump_marker(""), None);
    }

    #[test]
    fn bump_marker_is_none_when_the_value_is_not_a_number_with_a_semicolon() {
        assert_eq!(bump_marker("pub const BUILDTIME_MARKER: u64 = x;\n"), None);
        assert_eq!(bump_marker("pub const BUILDTIME_MARKER: u64 = 1\n"), None);
        assert_eq!(bump_marker("pub const BUILDTIME_MARKER: u64 = ;\n"), None);
    }

    #[test]
    fn bump_marker_is_none_when_the_number_overflows() {
        let text = format!("pub const BUILDTIME_MARKER: u64 = {};\n", u64::MAX);
        assert_eq!(bump_marker(&text), None);
    }

    #[test]
    fn in_crate_joins_a_directory_and_a_file() {
        assert_eq!(
            in_crate("crates/desktop", "src/lib.rs"),
            "crates/desktop/src/lib.rs"
        );
        assert_eq!(in_crate("", "src/lib.rs"), "src/lib.rs");
    }

    #[test]
    fn a_copy_without_a_marker_line_is_not_a_copy() {
        assert_eq!(
            leftover(Some("anything at all\n"), ".DS_Store junk"),
            Leftover::NotACopy
        );
        assert_eq!(leftover(None, ""), Leftover::NotACopy);
    }

    #[test]
    fn the_same_file_with_only_the_marker_bumped_is_restored() {
        let copy = "//! crate\npub const BUILDTIME_MARKER: u64 = 41;\n";
        let bumped = "//! crate\npub const BUILDTIME_MARKER: u64 = 148;\n";
        assert_eq!(leftover(Some(bumped), copy), Leftover::Restore);
        assert_eq!(leftover(Some(copy), copy), Leftover::Restore);
    }

    #[test]
    fn a_real_edit_elsewhere_is_a_conflict() {
        let copy = "//! crate\npub const BUILDTIME_MARKER: u64 = 41;\npub fn f() {}\n";
        let edited = "//! crate\npub const BUILDTIME_MARKER: u64 = 42;\npub fn f() { 1 }\n";
        assert_eq!(leftover(Some(edited), copy), Leftover::Conflict);
    }

    #[test]
    fn a_file_that_lost_its_marker_line_is_a_conflict() {
        let copy = "pub const BUILDTIME_MARKER: u64 = 41;\n";
        assert_eq!(leftover(Some("pub fn f() {}\n"), copy), Leftover::Conflict);
    }

    #[test]
    fn a_missing_file_is_missing() {
        let copy = "pub const BUILDTIME_MARKER: u64 = 41;\n";
        assert_eq!(leftover(None, copy), Leftover::Missing);
    }
}
