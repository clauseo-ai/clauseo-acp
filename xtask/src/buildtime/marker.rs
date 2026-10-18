//! The marker line each probe edits: where it is, what it holds, and the next value to write.

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
    let mut out = String::with_capacity(text.len() + 1);
    let mut found = false;
    for line in text.split_inclusive('\n') {
        match (found, bump_line(line)) {
            (false, Some(bumped)) => {
                out.push_str(&bumped);
                found = true;
            }
            _ => out.push_str(line),
        }
    }
    found.then_some(out)
}

/// The bumped form of one line, or `None` when the line is not the marker.
fn bump_line(line: &str) -> Option<String> {
    let body = line.trim_start();
    let indent = &line[..line.len() - body.len()];
    let rest = body.strip_prefix(PREFIX)?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let (number, tail) = rest.split_at(digits);
    if digits == 0 || !tail.starts_with(';') {
        return None;
    }
    let next = number.parse::<u64>().ok()?.checked_add(1)?;
    Some(format!("{indent}{PREFIX}{next}{tail}"))
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
}
