//! A download copied into a folder opened once (`write::Folder`), for a move the file system
//! cannot make: the folder on another drive than staging (`CrossesDevices`), or, on Unix, a
//! file system without hard links (FAT, exFAT). Every step names the file relative to the
//! folder's handle, never by the folder's path:
//! 1. the staging file is opened, refusing a link ([`Folder::copy_in`]);
//! 2. a hidden temporary file `.clauseo-dl-<random>` is made new in the folder (Unix
//!    `O_CREAT|O_EXCL|O_NOFOLLOW`, Windows `FILE_CREATE` relative to the folder's handle);
//! 3. the caller's [`Mark`] marks it, before a byte of the download is in it, so no reader ever
//!    sees the content without its mark; a mark that fails or does not read back ends the copy
//!    ([`CopyFailed::Mark`]) and the temporary file goes;
//! 4. the content is copied and flushed to the disk (`fsync`, `FlushFileBuffers`);
//! 5. [`Incoming::publish`] gives it its name without replacing anything (`AlreadyExists`
//!    otherwise, and the caller tries the next name);
//! 6. [`Incoming::remove_source`] deletes the staging file (on Windows through the handle opened
//!    in step 1, so a file held by another process fails step 1, not this one).
//!
//! Dropped before it is published, the temporary file is deleted.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::hash::{BuildHasher, Hasher};
use std::io;
#[cfg(unix)]
use std::io::Seek;
use std::path::Path;

use super::Folder;

/// Marks the new copy (the second file) as the download it copies (the first) is marked,
/// before any content is written, and checks the mark reads back.
pub(crate) type Mark = fn(&File, &File) -> io::Result<()>;

/// Why a copy failed.
#[derive(Debug)]
pub(crate) enum CopyFailed {
    /// The copy's drive does not keep the mark: no copy is left.
    Mark(io::Error),
    /// Anything else: the staging file did not open, the disk is full.
    Io(io::Error),
}

/// How many temporary names a copy tries when each is taken.
const TEMPORARY_TRIES: usize = 8;

/// The start of every temporary name.
const TEMPORARY_PREFIX: &str = ".clauseo-dl-";

/// A download copied into a folder under a hidden temporary name, waiting for its own.
pub(crate) struct Incoming<'a> {
    folder: &'a Folder,
    #[cfg(unix)]
    temporary: OsString,
    file: File,
    source: File,
    #[cfg(unix)]
    from: std::path::PathBuf,
    /// Unix: the mark again, for the copy made under the final name where the file system has
    /// no rename that never replaces.
    #[cfg(unix)]
    mark: Mark,
    published: bool,
}

/// [`Folder::copy_in`]: the file `from` copied into `folder` under a new temporary name,
/// marked by `mark` before any content, and flushed to the disk.
pub(super) fn copy_in<'a>(
    folder: &'a Folder,
    from: &Path,
    mark: Mark,
) -> Result<Incoming<'a>, CopyFailed> {
    #[cfg(unix)]
    let source = super::unix::open_source(from);
    #[cfg(windows)]
    let source = super::windows::open_source(from);
    let source = source.map_err(CopyFailed::Io)?;
    let (temporary, file) = make_temporary(folder).map_err(CopyFailed::Io)?;
    // Windows deletes or renames the temporary file through its handle, never its name.
    #[cfg(windows)]
    drop(temporary);
    let mut incoming = Incoming {
        folder,
        #[cfg(unix)]
        temporary,
        file,
        source,
        #[cfg(unix)]
        from: from.to_owned(),
        #[cfg(unix)]
        mark,
        published: false,
    };
    mark(&incoming.source, &incoming.file).map_err(CopyFailed::Mark)?;
    io::copy(&mut incoming.source, &mut incoming.file)
        .and_then(|_| incoming.file.sync_all())
        .map_err(CopyFailed::Io)?;
    Ok(incoming)
}

/// A new temporary file in `folder`: `.clauseo-dl-<16 hex digits>`, random (std's
/// `RandomState`, seeded by the OS), made with no-replace, no-follow semantics.
fn make_temporary(folder: &Folder) -> io::Result<(OsString, File)> {
    let mut last = io::Error::from(io::ErrorKind::AlreadyExists);
    for _ in 0..TEMPORARY_TRIES {
        let random = std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish();
        let name = OsString::from(format!("{TEMPORARY_PREFIX}{random:016x}"));
        match folder.create_new(&name) {
            Ok(file) => return Ok((name, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => last = error,
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

impl Incoming<'_> {
    /// Gives the copy `name` in the folder, never replacing anything there: `AlreadyExists` when
    /// it is taken. On Unix, where the file system has no rename that never replaces and no
    /// hard link, the copy is made again under `name` itself, new and marked first.
    pub(crate) fn publish(&mut self, name: &OsStr) -> io::Result<()> {
        #[cfg(unix)]
        let published = match super::unix::rename_new_in(&self.folder.dir, &self.temporary, name) {
            Err(error) if error.kind() == io::ErrorKind::Unsupported => self.copy_as(name),
            done => done,
        };
        #[cfg(windows)]
        let published = super::windows::rename_into(&self.file, &self.folder.dir, name);
        if published.is_ok() {
            self.published = true;
        }
        published
    }

    /// The copy made again under `name`: new (`O_EXCL|O_NOFOLLOW`), marked, then filled from
    /// the temporary file, which then goes.
    #[cfg(unix)]
    fn copy_as(&mut self, name: &OsStr) -> io::Result<()> {
        let dir = &self.folder.dir;
        let mut copy = super::unix::create_new_in(dir, name)?;
        let filled = (self.mark)(&self.source, &copy)
            .and_then(|()| self.file.seek(io::SeekFrom::Start(0)))
            .and_then(|_| io::copy(&mut self.file, &mut copy))
            .and_then(|_| copy.sync_all());
        if filled.is_err() {
            // `name` is the file just made, which nothing else had.
            drop(super::unix::remove_in(dir, name));
            return filled;
        }
        drop(super::unix::remove_in(dir, &self.temporary));
        Ok(())
    }

    /// Deletes the staging file the copy was made from.
    pub(crate) fn remove_source(self) -> io::Result<()> {
        #[cfg(unix)]
        {
            super::remove_file(&self.from)
        }
        #[cfg(windows)]
        {
            super::windows::delete_by_handle(&self.source)
        }
    }
}

impl Drop for Incoming<'_> {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        // Only this copy's own temporary file; one that cannot go is a hidden leftover.
        #[cfg(unix)]
        drop(super::unix::remove_in(&self.folder.dir, &self.temporary));
        #[cfg(windows)]
        drop(super::windows::delete_by_handle(&self.file));
    }
}
