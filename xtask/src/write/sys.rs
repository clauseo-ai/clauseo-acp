//! The std calls that change files, as thin wrappers and nothing else: the one place in xtask
//! (outside its tests) that may call them. Workspace `clippy.toml` bans each of them everywhere
//! else, and the expect below covers only this file, so the rest of the write door stays under
//! every other ban (`canonicalize`, `Command::status`, the clock, ...). A wrapper does exactly what
//! its std call does; the rules about which paths may change live in the door's other files.

#![expect(
    clippy::disallowed_methods,
    reason = "the write door's std calls live here alone; clippy.toml bans them elsewhere"
)]

use std::fs::{self, File, OpenOptions, Permissions};
use std::io;
use std::path::Path;

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

/// `fs::remove_dir`.
pub(super) fn remove_dir(path: &Path) -> io::Result<()> {
    fs::remove_dir(path)
}

/// `fs::remove_dir_all`.
pub(super) fn remove_dir_all(path: &Path) -> io::Result<()> {
    fs::remove_dir_all(path)
}

/// `fs::rename`.
pub(super) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    fs::rename(from, to)
}

/// `fs::hard_link`.
pub(super) fn hard_link(original: &Path, link: &Path) -> io::Result<()> {
    fs::hard_link(original, link)
}

/// `fs::set_permissions`.
pub(super) fn set_permissions(path: &Path, permissions: Permissions) -> io::Result<()> {
    fs::set_permissions(path, permissions)
}

/// `fs::write`.
pub(super) fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    fs::write(path, bytes)
}

/// `OpenOptions::open`.
pub(super) fn open(options: &OpenOptions, path: &Path) -> io::Result<File> {
    options.open(path)
}
