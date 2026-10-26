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
