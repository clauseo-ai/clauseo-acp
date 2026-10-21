//! The fix lines of the dependency graph reader, one variant per failure kind.
//! Each text is written once, in `Display`; `fix/tests.rs` pins every variant's text.

use crate::process::TimeoutFix;
use std::fmt;

/// A fix line of the dependency graph reader.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fix {
    /// `cargo tree` printed a line the reader does not know, or a shape it cannot read.
    TreeParse,
    /// `cargo` could not be started.
    CargoOnPath,
    /// `cargo` ran past its limit and was stopped.
    TimedOut(TimeoutFix),
    /// `cargo` fails because `Cargo.lock` does not match the manifests.
    Lockfile,
    /// A crate is locked but not downloaded, and the check runs offline.
    Fetch,
    /// A manifest names a package or version that does not exist.
    Manifest,
    /// Cargo's error fits both a wrong manifest and a crate not downloaded.
    Either,
    /// Another failure of `command`, the `cargo` run that failed.
    CargoReports { command: String },
}

impl fmt::Display for Fix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Fix::TreeParse => f.write_str(
                "update `Tree::parse` in xtask/src/checks/graph.rs to the lines this cargo prints, \
                 since the check reads each crate's dependency graph from them",
            ),
            Fix::CargoOnPath => f.write_str(
                "put `cargo` on PATH, since this check reads the dependency graph from cargo",
            ),
            Fix::TimedOut(fix) => fix.fmt(f),
            Fix::Lockfile => f.write_str(
                "Cargo.lock does not match the manifests, and the check never rewrites it. Run \
                 `cargo update --workspace` to bring it in line, then commit Cargo.lock",
            ),
            Fix::Fetch => f.write_str(
                "a crate is not downloaded and the check runs offline. Run `cargo fetch --locked`, \
                 then run the check again",
            ),
            Fix::Manifest => f.write_str(
                "cargo could not resolve the dependency that its error above names. Correct that \
                 dependency in the manifest of the package the error says requires it, then run \
                 the check again",
            ),
            Fix::Either => f.write_str(
                "cargo could not resolve the dependency that its error above names. If Cargo.lock \
                 lists that package, it is not downloaded and the check runs offline, so run \
                 `cargo fetch --locked`. If it does not, correct that dependency in the manifest \
                 of the package the error says requires it. Then run the check again",
            ),
            Fix::CargoReports { command } => write!(
                f,
                "fix what cargo reports above until `{command}` succeeds, since this check reads \
                 the dependency graph from it"
            ),
        }
    }
}

#[cfg(test)]
mod tests;
