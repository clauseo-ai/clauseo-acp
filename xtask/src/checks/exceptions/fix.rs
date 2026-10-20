//! The fix lines of the exception lists, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the exception lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// An entry with a path and no reason after it.
    WriteReason { path: String },
    /// A path listed a second time: the first line keeps its reason.
    DeleteDuplicate,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::WriteReason { path } => write!(
                f,
                "write why `{path}` is exempt after the path, on the same line"
            ),
            Fix::DeleteDuplicate => {
                f.write_str("delete this line and keep the reason on the first one")
            }
        }
    }
}

#[cfg(test)]
mod tests;
