//! The std calls that change files, as thin wrappers and nothing else: the one place these tests
//! change files outside `SystemFiles`. Workspace `clippy.toml` bans each of them, tests
//! included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the disk tests' setup calls std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::fs::{self, Permissions};
use std::io;
use std::path::Path;

/// A fresh, empty folder at `path`.
pub(crate) fn fresh_dir(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
        _ => {}
    }
    fs::create_dir_all(path)
}

/// `fs::write`.
pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}

/// `std::os::unix::fs::symlink`.
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// `fs::set_permissions`.
pub(crate) fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()> {
    fs::set_permissions(path, permissions)
}
