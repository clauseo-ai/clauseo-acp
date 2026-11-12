//! The fix lines of the unix-paths check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use super::{BASELINE, MARKER};
use std::fmt;

/// A fix line of the unix-paths check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A Unix absolute path in a test, with no baseline line of its own.
    UseAbs,
    /// A baseline line, at `line`, with no literal of its own.
    Stale { line: u32 },
    /// A baseline line that is not `<path> "<literal>"`.
    Malformed,
    /// A marker with no reason after it.
    GiveReason,
    /// A test file the lexer refuses.
    NotLexed,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::UseAbs => write!(
                f,
                "pass it through a helper that makes it absolute on every OS, such as the host's \
                 `crate::testing::abs(..)`; or keep the code off Windows with `#[cfg(unix)]`; or, \
                 when the path never reaches a path API, write `{MARKER} <reason>` on the line \
                 above"
            ),
            Fix::Stale { line } => write!(f, "delete line {line} of {BASELINE}"),
            Fix::Malformed => write!(
                f,
                "write the line as the repo-relative path, a space and the literal in double \
                 quotes, as the comment at the top of {BASELINE} says, or delete it"
            ),
            Fix::GiveReason => write!(
                f,
                "write why the path may stay Unix-only after `{MARKER}`, or delete the marker"
            ),
            Fix::NotLexed => f.write_str(
                "make the file valid Rust tokens: unix-paths lexes every test file for its string \
                 literals",
            ),
        }
    }
}

#[cfg(test)]
mod tests;
