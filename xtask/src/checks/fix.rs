//! The fix lines of the helpers in `checks.rs` that several checks call, one variant per failure
//! kind. Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of a helper in `checks.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A check inspected nothing (`StaticCheck::nothing_inspected`): what `name` looks for,
    /// `place` where, and `source`, the file whose consts build that place.
    NothingInspected {
        name: &'static str,
        many: &'static str,
        place: String,
        source: &'static str,
    },
    /// `cargo metadata` failed, so a check that reads the workspace members stopped.
    Metadata,
    /// The desktop crate is not a workspace member.
    DesktopNotMember,
    /// git could not start, so `check` could not list the repo's files.
    GitNotStarted { check: String },
    /// git ran and failed, so `check` could not list the repo's files.
    GitFailed { check: String },
    /// git ran past its limit, so `check` could not list the repo's files.
    GitTimedOut { check: String },
    /// A file is there but is not UTF-8 text.
    NotUtf8,
    /// A file or folder could not be read for another reason, which the message quotes.
    Unreadable,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::NothingInspected {
                name,
                many,
                place,
                source,
            } => write!(
                f,
                "{name} looks for {many} {place}; restore them, or, if they moved, update the \
                 consts {source} builds that place from"
            ),
            Fix::Metadata => f.write_str(
                "fix the error quoted above so that `cargo metadata` succeeds; this check reads \
                 the workspace members from its output",
            ),
            Fix::DesktopNotMember => f.write_str(
                "restore crates/desktop or update xtask/src/checks: these checks guard its \
                 capabilities, plugins and Tauri config",
            ),
            Fix::GitNotStarted { check } => write!(
                f,
                "install git and put it on PATH: {check} lists the files it reads with \
                 `git ls-files`"
            ),
            Fix::GitFailed { check } => write!(
                f,
                "run the check inside a git checkout, or fix what git reports: {check} lists the \
                 files it reads with `git ls-files`"
            ),
            Fix::GitTimedOut { check } => write!(
                f,
                "run that git command by hand to see why it does not finish: {check} lists the \
                 files it reads with `git ls-files`"
            ),
            Fix::NotUtf8 => f.write_str("make the file readable as UTF-8 text"),
            Fix::Unreadable => f.write_str(
                "fix what the error says so the check can read it: restore or replace the file \
                 or folder",
            ),
        }
    }
}

#[cfg(test)]
mod tests;
