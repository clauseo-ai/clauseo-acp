//! The fix lines of the log-literals check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the log-literals check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A `warn!` or `error!` whose message is formatted.
    Literal,
    /// A `.rs` file syn cannot parse.
    Unparsable,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::Literal => write!(
                f,
                "make the message a literal with no {{}} and put each value in a field: the \
                 runner's log keeps a literal message whole and drops a string field unless its \
                 name is one crates/sandbox/src/windows/runner/log_file.rs keeps"
            ),
            Fix::Unparsable => write!(
                f,
                "make the file valid Rust: log-literals parses it to find its warn! and error! \
                 calls"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
