//! The std calls that change files, as thin wrappers and nothing else: the one place the stdio
//! tests change files. Workspace `clippy.toml` bans each of them, tests included, and the expect
//! below covers only this file, so the rest of the tests stay under every other ban. Each wrapper
//! does exactly what its std call does; the tests' setup and cleanup decide which paths they touch.

#![expect(
    clippy::disallowed_methods,
    reason = "the stdio tests' file setup calls std's writes here alone; clippy.toml bans them elsewhere"
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

/// `fs::create_dir`.
pub(crate) fn create_dir(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: impl AsRef<Path>) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
