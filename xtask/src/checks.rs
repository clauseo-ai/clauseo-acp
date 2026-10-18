//! Static checks: Rust code that inspects the repo instead of running a tool.
//!
//! One file per check in `src/checks/`, registered in `ALL` below. `steps::plan` runs every entry
//! after the tool steps and before nextest.

mod budgets;
mod capabilities;
mod plugins;

use crate::violation::Violation;
use crate::workspace::Context;
use std::fs;
use std::io;
use std::path::Path;

/// A check written in Rust. It fails iff it returns violations.
pub(crate) struct StaticCheck {
    pub name: &'static str,
    pub run: fn(&Context) -> Vec<Violation>,
}

/// Every static check, in run order.
pub(crate) const ALL: &[StaticCheck] = &[budgets::CHECK, capabilities::CHECK, plugins::CHECK];

/// Reads `root/rel` as text. `Ok(None)` when the file does not exist; any other read error is a
/// violation at line 1.
pub(crate) fn read_repo_file(root: &Path, rel: &str) -> Result<Option<String>, Violation> {
    match fs::read_to_string(root.join(rel)) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(unreadable(rel, &error)),
    }
}

/// The violation for `path`, which exists but cannot be read as UTF-8 text.
pub(crate) fn unreadable(path: &str, error: &io::Error) -> Violation {
    Violation {
        path: path.to_owned(),
        line: 1,
        message: format!("could not read `{path}`: {error}"),
        fix: "make the file readable as UTF-8 text".to_owned(),
    }
}

/// `path` relative to `root`, with `/` separators.
pub(crate) fn repo_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The 1-based line number of the line at `index`, counting from 0 as `str::lines` does.
pub(crate) fn line_at(index: usize) -> u32 {
    u32::try_from(index.saturating_add(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_missing_file_reads_as_none_and_a_present_one_as_its_text() {
        let dir = Scratch::new("checks-read");
        assert_eq!(read_repo_file(&dir, "a.txt"), Ok(None));
        fs::write(dir.join("a.txt"), "hello").expect("write the file");
        assert_eq!(read_repo_file(&dir, "a.txt"), Ok(Some("hello".to_owned())));
    }

    #[test]
    fn a_path_is_repo_relative_with_slashes() {
        let root = Path::new("/ws");
        assert_eq!(
            repo_path(root, Path::new("/ws/crates/desktop/x.json")),
            "crates/desktop/x.json"
        );
    }

    #[test]
    fn line_numbers_start_at_one() {
        assert_eq!(line_at(0), 1);
        assert_eq!(line_at(4), 5);
    }
}
