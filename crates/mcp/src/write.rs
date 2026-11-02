//! The std call that changes files, as a thin wrapper and nothing else: the one place in
//! `clauseo-mcp` that may call it. Workspace `clippy.toml` bans it everywhere else, and the expect
//! below covers only this file. The guard decides which folders to make, in `browser_guard.rs`.

#![expect(
    clippy::disallowed_methods,
    reason = "the guard makes its child's folders here alone; clippy.toml bans std's writes elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::create_dir`.
pub(crate) fn create_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}
