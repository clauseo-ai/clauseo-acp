//! The fix lines of the buildtime-edits check, one variant per failure kind. Each text is written
//! once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the buildtime-edits check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// The attribute is left in a file and no live run holds the lock: an interrupted run left it.
    Leftover,
    /// The attribute is in a file while a `cargo xtask buildtime` run holds the lock: a live run.
    LiveRun,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Leftover => f.write_str(
                "run `cargo xtask buildtime web` once to restore the file from \
                 target/xtask/buildtime, or remove the attribute by hand",
            ),
            Fix::LiveRun => f.write_str("rerun the check when that run ends"),
        }
    }
}

#[cfg(test)]
mod tests;
