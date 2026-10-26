//! The std calls that change files, as thin wrappers: the one place in `matters` (outside its
//! tests and bench) that may call them. The workspace `clippy.toml` bans each of them everywhere
//! else, and the expect below covers only this file. [`replace`] is the one way a file in the
//! matter folder changes: a temp file beside it, synced, then renamed over it.

#![expect(
    clippy::disallowed_methods,
    reason = "matters writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::fsync;

/// `fs::create_dir`.
pub(crate) fn create_dir(path: &Path) -> io::Result<()> {
    std::fs::create_dir(path)
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// `fs::remove_dir` (an empty folder only).
pub(crate) fn remove_dir(path: &Path) -> io::Result<()> {
    std::fs::remove_dir(path)
}

/// `fs::remove_dir_all`.
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}

/// `fs::rename`.
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// A new file opened for writing; fails if `path` exists.
pub(crate) fn create_new(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// Opens an existing file for `fsync::sync_file`: read-only on Unix (loose objects are
/// read-only there), with write access on Windows, where `FlushFileBuffers` needs it.
pub(crate) fn open_to_sync(path: &Path) -> io::Result<File> {
    #[cfg(windows)]
    {
        OpenOptions::new().write(true).open(path)
    }
    #[cfg(not(windows))]
    {
        File::open(path)
    }
}

/// Windows: opens an existing file for reading with no sharing, which fails with a sharing
/// violation while another program has it open.
#[cfg(windows)]
pub(crate) fn open_exclusive(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;
    OpenOptions::new().read(true).share_mode(0).open(path)
}

/// The most temp names [`replace`] tries in one folder before giving up.
const TEMP_TRIES: u32 = 1000;

/// Replaces (or creates) `target` with what `fill` writes: a new temp file in the same folder,
/// named `.clauseo-<n>.tmp` (excluded from every checkpoint), synced, then renamed over
/// `target`, then the folder synced. Never truncates `target` in place. The temp file is
/// removed if anything fails before the rename.
pub(crate) fn replace(
    target: &Path,
    fill: &mut dyn FnMut(&mut File) -> Result<(), Error>,
) -> Result<(), Error> {
    let dir = target
        .parent()
        .ok_or_else(|| Error::io("replace", target, io::ErrorKind::InvalidInput.into()))?;
    let (temp, mut file) = new_temp(dir)?;
    let written = fill(&mut file)
        .and_then(|()| fsync::sync_file(&file).map_err(|err| Error::io("sync", &temp, err)));
    drop(file);
    if let Err(err) =
        written.and_then(|()| rename(&temp, target).map_err(|err| Error::io("rename", target, err)))
    {
        if let Err(cleanup) = remove_file(&temp) {
            tracing::warn!(path = %temp.display(), error = %cleanup, "temp file left behind");
        }
        return Err(err);
    }
    fsync::sync_dir(dir).map_err(|err| Error::io("sync", dir, err))
}

/// A new temp file in `dir`, by the first free name `.clauseo-<n>.tmp`.
fn new_temp(dir: &Path) -> Result<(PathBuf, File), Error> {
    for n in 0..TEMP_TRIES {
        let path = dir.join(format!(".clauseo-{n}.tmp"));
        match create_new(&path) {
            Ok(file) => return Ok((path, file)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Error::io("create", path, err)),
        }
    }
    Err(Error::io(
        "create",
        dir.join(".clauseo-0.tmp"),
        io::ErrorKind::AlreadyExists.into(),
    ))
}
