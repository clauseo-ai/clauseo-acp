//! The std calls that change files, as thin wrappers and nothing else: the one place the guard
//! tests change files. Workspace `clippy.toml` bans each of them, tests included, and the expect
//! below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the guard tests' scratch folders call std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::io;
use std::path::Path;

/// `fs::write`.
pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `fs::copy`.
pub(crate) fn copy(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<u64> {
    std::fs::copy(from, to)
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `fs::set_permissions` with a Unix mode.
#[cfg(unix)]
pub(crate) fn set_mode(path: impl AsRef<Path>, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// `os::unix::fs::symlink`.
#[cfg(unix)]
pub(crate) fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// `fs::canonicalize`, to compare the folder a child reports (symlinks resolved, as on macOS's
/// `/var`) with the one the test gave; both sides go through it.
pub(crate) fn resolved(path: impl AsRef<Path>) -> io::Result<std::path::PathBuf> {
    std::fs::canonicalize(path)
}
