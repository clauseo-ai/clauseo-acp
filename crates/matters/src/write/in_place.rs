//! A lawyer's file the rename may not replace, written in place instead (macOS; Linux in tests).
//!
//! macOS refuses a rename over a file whose ACL denies `delete` (`chmod +a "everyone deny
//! delete"`): replacing the name removes the old file. The lawyer protected the file from being
//! deleted, not from being changed, so its content is written through the file itself: the same
//! inode, so its ACL, extended attributes, birth time and mode stay as they are.
//!
//! What that gives up is the atomic replace. The write runs only after every check below has
//! passed, in this order, on the one open file:
//!
//! 1. Opened for reading and writing without following a symlink (`O_NOFOLLOW`; `O_NONBLOCK`, so
//!    a FIFO cannot hold the open), never truncated by the open.
//! 2. Identity: a regular file with exactly one link, whose device and inode are those of the name
//!    now (`lstat`). A second name (a hard link, maybe outside the matter) shares
//!    the inode, so a write in place would change that file too; a rename never does. Refused as
//!    [`RefusedBecause::SecondName`].
//! 3. The new content (the temp file) opened for reading.
//! 4. The saved copy: [`SavedCopy::confirm`] reads the file through this descriptor and checks
//!    it is the version the history holds, and that the history's copy reads back whole
//!    (`writeback`'s `confirm_saved`). Only then may its bytes be overwritten.
//!
//! Any of these failing leaves the file untouched; but for a permission denied at the open
//! ([`RefusedBecause::Permissions`]) and a second name, it is refused as
//! [`RefusedBecause::SavedCopyUnconfirmed`]: the file could not be confirmed as the one whose
//! saved copy the history holds.
//!
//! Then the temp file's content is written from offset 0, the file cut to its length, and synced
//! (the write-back's `F_FULLFSYNC` on the matter folder follows, as after its renames). A failure
//! there (a full disk, an I/O error, the cut or the sync failing) may leave the file torn: the
//! new content's first part over the old, the old content's tail, or a short file. So the
//! version confirmed in step 4 is written back through the same open file at once, cut to its
//! length and synced ([`SavedCopy::write_back`]): when that works the file is as it was, its
//! content byte for byte (its modification time is new). Either way the write fails with
//! [`NotWritten::Torn`], saying whether the file was put back; the history still holds the
//! version before (the caller's checkpoint before the write). A crash between the first write
//! and the sync leaves the file torn with nothing to put it back; the next checkpoint records
//! it as changed outside Clauseo, and the version before it is in Versions to restore.

use std::fs::{File, OpenOptions};
use std::io::{self, Read as _, Seek as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::path::{Path, PathBuf};

use super::rename::{RenameFailed, Renamed, SavedCopy};
use super::{Lost, Staged};
use crate::{Error, RefusedBecause};

/// After the rename of `temp` over `staged`'s target was refused (`refusal`): the target
/// written in place ([`write`]). The temp file goes as `staged` drops, whatever happened; the
/// target keeps its own ACL and mode, so nothing waits for the step after a rename.
pub(super) fn write_in_place(
    mut staged: Staged,
    temp: PathBuf,
    dir: PathBuf,
    refusal: io::Error,
    saved: &mut dyn SavedCopy,
) -> Result<Renamed, RenameFailed> {
    let written = write(&temp, &staged.target, saved);
    staged.temp = Some(temp);
    #[cfg(target_os = "macos")]
    {
        staged.after = None;
    }
    match written {
        Ok(()) => Ok(Renamed {
            dir,
            lost: Lost::default(),
            owner_only: false,
        }),
        Err(NotWritten::Refused(because)) => Err(RenameFailed {
            error: Error::io("rename", &staged.target, refusal),
            changed: false,
            refused: Some(because),
            torn: None,
        }),
        Err(NotWritten::Torn { error, put_back }) => Err(RenameFailed {
            error: Error::io("write in place", &staged.target, error),
            changed: !put_back,
            refused: None,
            torn: Some(put_back),
        }),
    }
}

/// Why [`write`] did not write.
#[derive(Debug)]
pub(super) enum NotWritten {
    /// Refused for this reason before a byte was written; the file is as it was.
    Refused(RefusedBecause),
    /// Failed with `error` after the write began: the file may have been torn, and was put back
    /// as it was when `put_back` (see the module docs).
    Torn {
        /// The write's failure.
        error: io::Error,
        /// The confirmed version was written back whole and synced.
        put_back: bool,
    },
}

/// Writes the content of the file `temp` over the file `target` in place, after the checks in the
/// module docs; `saved` is step 4, and puts a torn file back.
pub(super) fn write(
    temp: &Path,
    target: &Path,
    saved: &mut dyn SavedCopy,
) -> Result<(), NotWritten> {
    let mut file = open(target).map_err(|err| {
        if err.kind() == io::ErrorKind::PermissionDenied {
            NotWritten::Refused(RefusedBecause::Permissions)
        } else {
            unconfirmed("open", &err)
        }
    })?;
    let opened = file.metadata().map_err(|err| unconfirmed("stat", &err))?;
    #[cfg(test)]
    super::seams::opened();
    if !opened.is_file() {
        return Err(unconfirmed("open", &io::Error::other("not a regular file")));
    }
    if opened.nlink() != 1 {
        return Err(NotWritten::Refused(RefusedBecause::SecondName));
    }
    let named = std::fs::symlink_metadata(target).map_err(|err| unconfirmed("stat", &err))?;
    if named.dev() != opened.dev() || named.ino() != opened.ino() {
        let err = io::Error::other("another file since it was opened");
        return Err(unconfirmed("stat", &err));
    }
    let mut from = super::open_read_nofollow(temp).map_err(|err| unconfirmed("open new", &err))?;
    // The safety net for the non-atomic write below: the bytes about to be overwritten must be
    // the version the history holds, and the history's copy must read back whole (hashed to its
    // id). Without that, a failure mid-write could lose the lawyer's file for good.
    if !saved.confirm(&mut file) {
        return Err(NotWritten::Refused(RefusedBecause::SavedCopyUnconfirmed));
    }
    file.rewind().map_err(|err| unconfirmed("seek", &err))?;
    #[cfg(test)]
    super::injected("in place").map_err(|err| unconfirmed("write", &err))?;
    copy(&mut from, &mut file).map_err(|error| NotWritten::Torn {
        error,
        put_back: put_back(&mut file, saved),
    })
}

/// A check of [`write`] that failed with `err` at `op`, before any byte was written: the file
/// is refused as unconfirmed. Logged with the op and the error's kind, never a path.
fn unconfirmed(op: &'static str, err: &io::Error) -> NotWritten {
    tracing::warn!(op, kind = ?err.kind(), "a file to write in place not confirmed");
    NotWritten::Refused(RefusedBecause::SavedCopyUnconfirmed)
}

/// After a write in place failed part way: the version `saved` confirmed written back through
/// `file` from the start, the file cut to its length and synced. Whether that worked, so the
/// file is as it was; a failure is logged by its kind, never a path.
fn put_back(file: &mut File, saved: &mut dyn SavedCopy) -> bool {
    let mut again = || -> io::Result<()> {
        file.rewind()?;
        #[cfg(test)]
        super::injected("put back")?;
        let len = saved.write_back(file)?;
        file.set_len(len)?;
        crate::fsync::sync_file(file)
    };
    match again() {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(kind = ?err.kind(), "a file written in place part way not put back");
            false
        }
    }
}

/// Opens `target` for reading and writing, not through a symlink, a FIFO not waiting for a
/// writer; the open leaves the content alone.
fn open(target: &Path) -> io::Result<File> {
    use rustix::fs::{OFlags, fcntl_getfl, fcntl_setfl};
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags((OFlags::NOFOLLOW | OFlags::NONBLOCK).bits().cast_signed())
        .open(target)?;
    fcntl_setfl(&file, fcntl_getfl(&file)?.difference(OFlags::NONBLOCK))?;
    Ok(file)
}

/// Copies all of `from` to `to` at its position (the start), cuts `to` to that length, and
/// syncs it.
fn copy(from: &mut File, to: &mut File) -> io::Result<()> {
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut len = 0_u64;
    loop {
        let read = from.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        to.write_all(buffer.get(..read).unwrap_or_default())?;
        len += u64::try_from(read).unwrap_or(u64::MAX);
        #[cfg(test)]
        super::injected("in place torn")?;
    }
    #[cfg(test)]
    super::injected("in place cut")?;
    to.set_len(len)?;
    #[cfg(test)]
    super::injected("in place sync")?;
    crate::fsync::sync_file(to)
}
