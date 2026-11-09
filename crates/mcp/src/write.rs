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

/// Reads `file` whole, refusing a link at the name: on Unix `O_NOFOLLOW` (a symlink fails with
/// `ELOOP`; `O_NONBLOCK` keeps a FIFO from blocking the open), on Windows the reparse point
/// itself (`FILE_FLAG_OPEN_REPARSE_POINT`); then anything but a plain file is refused. The kept
/// profile's `Default/Preferences` and `Local State` (`browser_guard/chrome/profile.rs`).
pub(crate) fn read_no_follow(file: &Path) -> io::Result<Vec<u8>> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        /// `FILE_FLAG_OPEN_REPARSE_POINT`.
        const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(OPEN_REPARSE_POINT);
    }
    let mut opened = options.open(file)?;
    if !opened.metadata()?.is_file() {
        return Err(io::Error::other("not a plain file"));
    }
    let mut bytes = Vec::new();
    io::Read::read_to_end(&mut opened, &mut bytes)?;
    Ok(bytes)
}

/// `UnixListener::bind` at a path, for the tests' stand-in guards; the guard's own socket is
/// [`bind_unix_socket_in`].
#[cfg(all(test, unix))]
pub(crate) fn bind_unix_socket(path: &Path) -> io::Result<std::os::unix::net::UnixListener> {
    std::os::unix::net::UnixListener::bind(path)
}

#[cfg(unix)]
pub(crate) use unix::{bind_unix_socket_in, remove_in};

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

/// `fs::rename`, replacing `to`: a guard run's Chrome record (`browser_guard/record.rs`), written
/// to a temporary name first, so a reader sees the old record or the new one, never half.
pub(crate) fn replace(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// `fs::remove_file`: a guard run's lock file, once its profile is gone.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `fs::remove_dir`: a link to a folder (a Windows junction) where Chrome's profile needs a real
/// folder (`browser_guard/chrome/profile.rs`); it removes the link, never what it points at.
pub(crate) fn remove_dir(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
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

/// Writes `contents` to `file` through a new temporary file beside it, renamed over `file`,
/// never writing through a link at `file`'s name: the kept Chrome profile's `Default/Preferences`
/// and `Local State` (`browser_guard/chrome/profile.rs`). Temporary files an earlier write left
/// (one killed midway) are removed first, and each write's own name is new ([`temporary_name`]),
/// so a leftover never fails a later write. On Unix every step is relative to `file`'s folder,
/// opened once with `O_NOFOLLOW` (`write/unix.rs`: `openat`, `renameat`); elsewhere each step
/// resolves the path again (`create_new`, then `fs::rename`).
pub(crate) fn replace_beside(file: &Path, contents: &[u8]) -> io::Result<()> {
    let name = file
        .file_name()
        .ok_or_else(|| io::Error::other("no file name"))?;
    let folder = file.parent().ok_or_else(|| io::Error::other("no folder"))?;
    #[cfg(unix)]
    {
        unix::replace_in_folder(folder, name, contents)
    }
    #[cfg(not(unix))]
    {
        use std::io::Write;
        let prefix = temporary_prefix(name);
        if let Ok(entries) = std::fs::read_dir(folder) {
            for entry in entries.flatten() {
                if entry
                    .file_name()
                    .as_encoded_bytes()
                    .starts_with(prefix.as_encoded_bytes())
                {
                    // Gone already, or not a file: left, harmless.
                    drop(std::fs::remove_file(entry.path()));
                }
            }
        }
        let temporary = folder.join(temporary_name(name));
        let mut new = std::fs::File::create_new(&temporary)?;
        let written = new.write_all(contents);
        drop(new);
        let replaced = written.and_then(|()| std::fs::rename(&temporary, file));
        if replaced.is_err() {
            // Only this write's own temporary file; one that cannot go now goes next time.
            drop(std::fs::remove_file(&temporary));
        }
        replaced
    }
}

/// The start of every temporary name [`replace_beside`] gives `name`: `.<name>.clauseo-new`.
fn temporary_prefix(name: &std::ffi::OsStr) -> std::ffi::OsString {
    let mut prefix = std::ffi::OsString::from(".");
    prefix.push(name);
    prefix.push(".clauseo-new");
    prefix
}

/// One write's own temporary name: the prefix, this process's id and a number no earlier call
/// in this process had, `.<name>.clauseo-new-<pid>-<n>`.
fn temporary_name(name: &std::ffi::OsStr) -> std::ffi::OsString {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut temporary = temporary_prefix(name);
    temporary.push(format!("-{}-{n}", std::process::id()));
    temporary
}

#[cfg(unix)]
mod unix;

/// `setxattr`: a download's `com.apple.quarantine` mark (`browser_guard/downloads/marks.rs`),
/// on the file itself, never through a symlink.
#[cfg(target_os = "macos")]
pub(crate) use mac::set_xattr;

/// `getxattr`, a read beside the write above: whether the download has its quarantine mark.
#[cfg(target_os = "macos")]
pub(crate) use mac::has_xattr;

#[cfg(all(target_os = "macos", test))]
pub(crate) use mac::read_xattr;

#[cfg(target_os = "macos")]
mod mac;

/// A file opened with no sharing at all, as a program that holds it open would, for the unit
/// tests' fixtures: Windows then refuses to move or delete it.
#[cfg(all(test, windows))]
pub(crate) fn open_unshared(file: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(0)
        .open(file)
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
