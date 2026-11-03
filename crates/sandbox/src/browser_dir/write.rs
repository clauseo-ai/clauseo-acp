//! The browser socket folder's file changes: its lock file, its `mkdir`, and their removal.
//! The one place that module changes files; workspace `clippy.toml` bans each call, and the
//! expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the browser socket folder's mkdir and removal, outside the jail, happen here alone"
)]

use std::fs::DirBuilder;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::fs::DirBuilderExt;
use std::path::Path;

use rustix::fs::{Mode, OFlags};

/// Makes the lock file `path` at mode 0600 and opens it: `O_CREAT|O_EXCL|O_NOFOLLOW`, so it
/// fails if anything, a link included, is there already.
pub(super) fn make_lock(path: &Path) -> io::Result<OwnedFd> {
    let flags = OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::RDWR | OFlags::CLOEXEC;
    Ok(rustix::fs::open(path, flags, Mode::RUSR | Mode::WUSR)?)
}

/// One `mkdir` of `path` at `mode` (less the umask); fails if anything is there already.
pub(super) fn make_dir(path: &Path, mode: u32) -> io::Result<()> {
    DirBuilder::new().mode(mode).create(path)
}

/// Removes the folder `path` and what is in it; a link at `path` is removed, not followed.
pub(super) fn remove_tree(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// Removes the empty folder `path`.
pub(super) fn remove_empty(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// Removes the file `path` (a link itself, never what it points to).
pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}
