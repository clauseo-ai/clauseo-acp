//! The sync calls, one per OS family, so the history's sync order and the folder writes share
//! them. gix 0.89 syncs no loose object, so this crate does.
//!
//! - macOS: [`sync_file`] is a plain `fsync`, which reaches the drive but not past its cache;
//!   [`barrier`] is `F_BARRIERFSYNC` (writes before it reach the disk before writes after it);
//!   [`full_flush`] is `F_FULLFSYNC`, which empties the drive's cache. `File::sync_all` is
//!   `F_FULLFSYNC` there, so it is never called per file.
//! - Linux and Windows: [`sync_file`] is `File::sync_all`; [`barrier`] and [`full_flush`] do
//!   nothing more, since each file was already synced in full.
//! - [`sync_dir`] syncs a folder's entries (a rename or a new file) on Unix; on Windows a folder
//!   cannot be opened as a file without extra flags, and NTFS journals the rename itself.
//! - [`full_flush_dir`] is [`full_flush`] on a matter folder, which may sit on a file system
//!   without `F_FULLFSYNC` (an SMB or NFS share, exFAT); there it settles for the folder syncs
//!   already done, as Go and SQLite fall back to a plain fsync.

use std::fs::File;
use std::io;
use std::path::Path;

/// Makes `file`'s data and metadata reach the drive (macOS: not past its cache).
pub(crate) fn sync_file(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        nix::unistd::fsync(file).map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        file.sync_all()
    }
}

#[cfg_attr(
    not(unix),
    expect(
        clippy::unnecessary_wraps,
        reason = "Unix syncs the folder; Windows has nothing to do"
    )
)]
/// Makes the entries of folder `dir` (new names, renames) reach the drive; nothing on Windows.
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        sync_file(&File::open(dir)?)
    }
    #[cfg(not(unix))]
    {
        let _ = dir;
        Ok(())
    }
}

#[cfg_attr(
    not(target_os = "macos"),
    expect(clippy::unnecessary_wraps, reason = "only macOS has a barrier to call")
)]
/// macOS: `F_BARRIERFSYNC` on `file`, so everything synced before reaches the disk before
/// anything written after. Elsewhere nothing: each file was synced in full.
pub(crate) fn barrier(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_BARRIERFSYNC)
            .map(drop)
            .map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = file;
        Ok(())
    }
}

#[cfg_attr(
    not(target_os = "macos"),
    expect(
        clippy::unnecessary_wraps,
        reason = "only macOS has a full flush to call"
    )
)]
/// macOS: `F_FULLFSYNC` on `file`, which empties the drive's cache, so everything synced so far
/// is on permanent storage. Elsewhere nothing: each file was synced in full.
pub(crate) fn full_flush(file: &File) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        nix::fcntl::fcntl(file, nix::fcntl::FcntlArg::F_FULLFSYNC)
            .map(drop)
            .map_err(io::Error::from)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = file;
        Ok(())
    }
}

/// macOS: [`full_flush`] on folder `dir`, whose entries were already synced with [`sync_dir`].
/// A file system that does not offer `F_FULLFSYNC` is not an error ([`settle_full_flush`]).
/// Elsewhere nothing.
pub(crate) fn full_flush_dir(dir: &Path) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let result = File::open(dir).and_then(|file| full_flush(&file));
    #[cfg(not(target_os = "macos"))]
    let result = Ok(());
    settle_full_flush(dir, result)
}

/// Darwin's `ENOTSUP`, `ENOTTY` and `EINVAL` (`<sys/errno.h>`): what `fcntl(F_FULLFSYNC)`
/// returns on a file system without it. Fixed numbers, not the build target's, so the decision
/// is tested on every OS.
const DARWIN_NO_FULL_FLUSH: [i32; 3] = [45, 25, 22];

/// `result` of a full flush of `dir`, with the errors that mean "this file system has no
/// `F_FULLFSYNC`" turned into `Ok`: the folder syncs before it are as far as such a drive goes.
/// Any other error is returned.
fn settle_full_flush(dir: &Path, result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(err)
            if err
                .raw_os_error()
                .is_some_and(|code| DARWIN_NO_FULL_FLUSH.contains(&code)) =>
        {
            tracing::debug!(path = %dir.display(), error = %err, "no F_FULLFSYNC here; the folder syncs stand");
            Ok(())
        }
        other => other,
    }
}

#[cfg(test)]
mod tests;
