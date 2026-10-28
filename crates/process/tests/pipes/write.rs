//! The std calls that change files, as thin wrappers and nothing else: the one place the pipe
//! tests change files (each run's own folder under the temp folder). Workspace `clippy.toml` bans
//! them, tests included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the pipe tests' run folders are made and removed here alone; clippy.toml bans it elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
