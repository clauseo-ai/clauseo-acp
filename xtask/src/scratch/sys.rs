//! The std calls that change files, as thin wrappers and nothing else: the one place in xtask's
//! tests that may call them. Workspace `clippy.toml` bans each of them everywhere else, and the
//! expect below covers only this file, so `Scratch`'s checks stay under every other ban. Each
//! wrapper does exactly what its std call does; `Scratch` checks the path first.

use std::fs;
#[cfg(unix)]
use std::fs::{File, OpenOptions, Permissions};
use std::io;
use std::path::Path;

/// `fs::write`.
pub(super) fn write(path: &Path, contents: &[u8]) -> io::Result<()> {
    fs::write(path, contents)
}

/// `fs::create_dir`.
pub(super) fn create_dir(path: &Path) -> io::Result<()> {
    fs::create_dir(path)
}

/// `fs::create_dir_all`.
pub(super) fn create_dir_all(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)
}

/// `fs::remove_file`.
pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    fs::remove_file(path)
}

/// `fs::remove_dir_all`.
pub(super) fn remove_dir_all(path: &Path) -> io::Result<()> {
    fs::remove_dir_all(path)
}

/// `fs::copy`.
pub(super) fn copy(from: &Path, to: &Path) -> io::Result<u64> {
    fs::copy(from, to)
}

/// `fs::set_permissions`.
#[cfg(unix)]
pub(super) fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()> {
    fs::set_permissions(path, permissions)
}

/// `OpenOptions::open` with write access and nothing else.
#[cfg(unix)]
pub(super) fn open_for_writing(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).open(path)
}

/// `std::os::unix::fs::symlink`.
#[cfg(unix)]
pub(super) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// `std::os::windows::fs::symlink_dir`.
#[cfg(windows)]
pub(super) fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}
