//! The std calls that change files, as thin wrappers and nothing else: the one place in
//! `clauseo-mcp` that may call them. Workspace `clippy.toml` bans them everywhere else, and the
//! expect below covers only this file. The guard decides which folders to make, in
//! `browser_guard.rs`.

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

/// Opens `file` read-only (`OpenOptions` is banned because it can also write); on Windows a
/// reparse point at the name is opened itself, never followed (`FILE_FLAG_OPEN_REPARSE_POINT`).
/// The port-file check (`browser_guard/port_file.rs`) reads agent-browser's port files with it.
pub(crate) fn open_read_no_follow(file: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_OPEN_REPARSE_POINT`.
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(OPEN_REPARSE_POINT);
    }
    options.open(file)
}

/// `fs::write`, for the unit tests' fixtures.
#[cfg(test)]
pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `fs::remove_dir_all`, for the unit tests' fixtures.
#[cfg(test)]
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `os::unix::fs::symlink`, for the unit tests' fixtures.
#[cfg(all(test, unix))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

/// `fs::set_permissions` with a Unix mode, for the unit tests' fixtures.
#[cfg(all(test, unix))]
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}
