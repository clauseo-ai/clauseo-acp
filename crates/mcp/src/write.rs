//! The std calls that change files, as thin wrappers and nothing else: the one place in
//! `clauseo-mcp` that may call them. Workspace `clippy.toml` bans them everywhere else, and the
//! expect below covers only this file. The guard decides which folders to make, in
//! `browser_guard.rs`, and what Chrome's profile holds, in `browser_guard/chrome.rs`.

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

/// `fs::create_dir_all`: Chrome's profile folders (`browser_guard/chrome.rs`).
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::write`: Chrome's `Default/Preferences` (`browser_guard/chrome.rs`).
pub(crate) fn write(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    std::fs::write(path, contents)
}

/// `fs::remove_dir_all`: the session's Chrome profile, at the guard's exit.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `File::create_new`: a guard run's lock file (`browser_guard/runs.rs`), which must be new.
pub(crate) fn create_new(path: &Path) -> io::Result<std::fs::File> {
    std::fs::File::create_new(path)
}

/// Opens `path` for writing, making it if missing and never truncating it: the lock file all
/// guard runs under one `--cwd` share (`browser_guard/runs.rs`), which holds nothing.
pub(crate) fn open_or_create(path: &Path) -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// `fs::remove_file`: a guard run's lock file, once its profile is gone.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// A finished download into the downloads folder (`browser_guard/downloads.rs`): moves the file
/// `from` to `to` within one file system, failing with `AlreadyExists` rather than replacing
/// anything at `to` (`fs::rename` replaces a file on every OS). On Unix `fs::hard_link`, which
/// never replaces, then `fs::remove_file` of `from` (or, if that fails, of the new link, so
/// the file keeps one name and the move can be tried again); on Windows `MoveFileExW` without
/// `MOVEFILE_REPLACE_EXISTING` (`write/windows.rs`). A file system without hard links refuses
/// the Unix move every time; the file then stays where it is.
pub(crate) fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::fs::hard_link(from, to)?;
        if let Err(error) = std::fs::remove_file(from) {
            // `to` is the link just made, which nothing else had.
            drop(std::fs::remove_file(to));
            return Err(error);
        }
        Ok(())
    }
    #[cfg(windows)]
    {
        windows::move_new(from, to)
    }
}

#[cfg(windows)]
mod windows;

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
