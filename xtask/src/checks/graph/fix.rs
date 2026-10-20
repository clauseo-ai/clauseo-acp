//! The fix lines of the dependency graph reader, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use std::fmt;

/// A fix line of the dependency graph reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {}

impl fmt::Display for Fix {
    fn fmt(&self, _f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {}
    }
}
