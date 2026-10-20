//! The fix lines of the file-size check, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use super::{COLUMN_LIMIT, EXCEPTIONS};
use std::fmt;

/// A fix line of the file-size check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A source file over the line limit that is not an exception.
    SplitOrExempt,
    /// A comment in a `.rs` file that reaches past the column limit.
    WrapComment,
    /// An exception that no longer earns its line.
    DeleteException,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::SplitOrExempt => write!(
                f,
                "split it into smaller modules, or list it in {EXCEPTIONS} with a reason"
            ),
            Fix::WrapComment => write!(f, "wrap the comment at {COLUMN_LIMIT} columns"),
            Fix::DeleteException => write!(f, "delete this line from {EXCEPTIONS}"),
        }
    }
}

#[cfg(test)]
mod tests;
