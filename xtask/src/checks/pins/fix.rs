//! The fix line of each failure `pins` reports, and its text. The helper that writes a home's
//! words (`unread_fix`) is a method of `FileHome` in `pins.rs`.

use super::{FileHome, SETUP, TOOLCHAIN};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// A `rust-version` whose major.minor is not the toolchain's.
    RustVersion { channel_major_minor: String },
    /// A `lefthook.yml` comment that names another lefthook version than `BINARIES`.
    LefthookComment { pinned: String },
    /// A home value that could not be read.
    Unread(FileHome),
    /// A YAML file that does not parse.
    NotYaml,
    /// A manifest that does not parse.
    NotToml,
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::RustVersion {
                channel_major_minor,
            } => write!(
                f,
                "set rust-version to \"{channel_major_minor}\", the major.minor of `channel` in \
                 {TOOLCHAIN}, the one home of the Rust version"
            ),
            Fix::LefthookComment { pinned } => write!(
                f,
                "check that the comment holds for lefthook {pinned}, then make it name {pinned}, \
                 the version `BINARIES` in {SETUP} pins"
            ),
            Fix::Unread(home) => home.unread_fix(f),
            Fix::NotYaml => write!(
                f,
                "make the file valid YAML at this line: pins parses it to find the versions it \
                 holds"
            ),
            Fix::NotToml => write!(
                f,
                "make the manifest valid TOML at this line: pins reads its rust-version there"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
