//! The policy's small value types: the browser's loopback reach and a path rule.

use std::path::{Path, PathBuf};

use super::{same, under};

/// The browser's loopback reach (agent-browser's and Chrome's CDP ports).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Loopback {
    /// No loopback beyond the proxy ports.
    #[default]
    None,
    /// These loopback ports, with bind and inbound on localhost. Sorted and de-duplicated.
    Ports(Vec<u16>),
    /// Any loopback port. Also reaches other loopback services and other launches' proxies,
    /// which refuse a request without their token (S16).
    Any,
}

/// A path rule: the path alone, or the path and everything under it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum PathRule {
    /// Exactly this path.
    Literal(PathBuf),
    /// This path and everything under it.
    Subpath(PathBuf),
}

impl PathRule {
    /// The rule's path.
    pub fn path(&self) -> &Path {
        match self {
            PathRule::Literal(path) | PathRule::Subpath(path) => path,
        }
    }

    /// Whether the rule covers `path`, by [`under`]: on macOS without case and in any Unicode
    /// normalisation form, as the kernel resolves names on a case-insensitive volume.
    pub fn covers(&self, path: &Path) -> bool {
        match self {
            PathRule::Literal(rule) => same(path, rule),
            PathRule::Subpath(rule) => under(path, rule),
        }
    }
}
