//! The std calls that change files, as thin wrappers and nothing else: the one place these tests
//! change files. Workspace `clippy.toml` bans each of them, tests included, and the expect below
//! covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the skills tests' setup calls std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::fs;
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

/// `fs::write`, making the folder first.
pub(crate) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, contents)
}

/// `fs::copy`.
pub(crate) fn copy(from: &Path, to: &Path) -> io::Result<()> {
    fs::copy(from, to).map(|_| ())
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// `fs::remove_dir_all`; a missing folder is no error.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// A symlink at `link` to the folder `target`.
#[cfg(unix)]
pub(crate) fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
