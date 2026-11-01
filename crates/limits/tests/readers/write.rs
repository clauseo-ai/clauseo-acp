//! The std calls that change files, as thin wrappers and nothing else: the one place the readers
//! tests change files (each test's own folder under the temp folder). Workspace `clippy.toml`
//! bans them, tests included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the readers tests change their folders here alone; clippy.toml bans it elsewhere"
)]

use std::io;
use std::path::Path;
use std::time::SystemTime;

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

/// Sets the modification time of the file `path`.
pub(crate) fn set_modified(path: &Path, time: SystemTime) -> io::Result<()> {
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_modified(time)
}

/// `os::unix::fs::symlink`.
#[cfg(unix)]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
