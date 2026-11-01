//! The std calls that change files, as thin wrappers and nothing else: the one place G16's test
//! crates (`tests/live.rs`, `tests/fixtures.rs`) change files, each in its own folder under the
//! temp folder or under `target/g16/`. Workspace `clippy.toml` bans them, tests included, and the
//! expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "G16's tests change their folders here alone; clippy.toml bans it elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::write`.
pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
