//! Where writeback's deletes go: to a trash first ([`Settings::trash`]), so the lawyer can take
//! the file back from there, and deleted for good only when the trash cannot take it and the
//! matter's history holds the file as the folder has it now. Undo and a restore that undoes an
//! earlier restore are the only callers (through `writeback::apply`), and each saves the folder
//! as its `outside-edits` checkpoint first, failing before any delete if that save fails.
//!
//! The OS's trash ([`OsTrash`]): the Trash on macOS, through `NSFileManager`'s
//! `trashItemAtURL` alone (never through Finder, which asks for Automation permission),
//! and the Recycle Bin on Windows, through the shell's `IFileOperation` with no UI, refused on a
//! volume without a Recycle Bin. Linux has none here (the app does not ship there): a plain
//! delete, as before. Tests use a plain folder instead ([`FolderTrash`]), so no run fills the
//! machine's Trash.
//!
//! [`Settings::trash`]: crate::Settings::trash

use std::ffi::OsString;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::MatterPath;

use crate::checkpoint::Core;
use crate::history::HistoryStore;
use crate::stat::EntryState;
use crate::write;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[cfg(target_os = "macos")]
mod macos_tests;

#[cfg(test)]
#[cfg(windows)]
mod windows_tests;

/// Moves a file out of the matter folder into a trash, where the lawyer can take it back.
pub trait Trash: Send + Sync {
    /// Moves the file at `path` (absolute; a regular file, not through a symlink, as writeback
    /// checked) into the trash, never deleting it for good. Returns where it went, when known.
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, TrashError>;
}

/// Why a [`Trash`] did not take a file.
#[derive(Debug)]
pub enum TrashError {
    /// This OS has no trash the app uses (Linux): the file is deleted, with no warning.
    Unsupported,
    /// The file's volume has no trash (a network share, a removable or exFAT drive), or the
    /// trash would have deleted it for good.
    NoTrash,
    /// The move failed; `NotFound` when nothing is at the path.
    Failed(io::Error),
}

impl fmt::Display for TrashError {
    /// The kind of failure only, never a path: what the warning logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("no trash on this OS"),
            Self::NoTrash => f.write_str("no trash on the volume"),
            Self::Failed(err) => write!(f, "{}", err.kind()),
        }
    }
}

/// The OS's trash: the Trash on macOS, the Recycle Bin on Windows; none on Linux
/// ([`TrashError::Unsupported`]). What the app uses.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsTrash;

impl Trash for OsTrash {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, TrashError> {
        #[cfg(target_os = "macos")]
        {
            macos::trash(path)
        }
        #[cfg(windows)]
        {
            windows::trash(path)
        }
        #[cfg(not(any(target_os = "macos", windows)))]
        {
            let _ = path;
            Err(TrashError::Unsupported)
        }
    }
}

/// A plain folder each file is moved into, as `<n>-<name>`: tests' trash, so they never fill
/// the machine's own. A file on another volume fails to move ([`TrashError::Failed`]).
#[derive(Debug)]
pub struct FolderTrash {
    folder: PathBuf,
    next: AtomicU64,
}

impl FolderTrash {
    /// The trash `folder`, made when the first file goes in.
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            next: AtomicU64::new(0),
        }
    }

    /// The folder.
    pub fn folder(&self) -> &Path {
        &self.folder
    }
}

impl Trash for FolderTrash {
    fn trash(&self, path: &Path) -> Result<Option<PathBuf>, TrashError> {
        let name = path
            .file_name()
            .ok_or_else(|| TrashError::Failed(io::ErrorKind::InvalidInput.into()))?;
        write::create_dir_all(&self.folder).map_err(TrashError::Failed)?;
        loop {
            let n = self.next.fetch_add(1, Ordering::Relaxed);
            let mut entry = OsString::from(format!("{n}-"));
            entry.push(name);
            let to = self.folder.join(entry);
            match std::fs::symlink_metadata(&to) {
                Ok(_) => continue,
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(TrashError::Failed(err)),
            }
            // A missing file fails here with `NotFound`, as a delete would.
            write::rename(path, &to).map_err(TrashError::Failed)?;
            return Ok(Some(to));
        }
    }
}

/// How a writeback's deletes went, for the log: counts, never paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Removals {
    /// Files moved to the trash.
    pub trashed: usize,
    /// Files deleted for good: the trash could not take them (or the OS has none the app
    /// uses), and the history holds each as the folder had it.
    pub deleted: usize,
}

/// One writeback's deletes. Dropped, it logs one warning if any file was deleted for good
/// after the trash failed, with the count and the first failure's kind, never a path.
pub(crate) struct Removing<'a> {
    trash: &'a dyn Trash,
    pub(crate) removals: Removals,
    fell_back: usize,
    first_failure: Option<String>,
}

impl<'a> Removing<'a> {
    pub(crate) fn new(trash: &'a dyn Trash) -> Self {
        Self {
            trash,
            removals: Removals::default(),
            fell_back: 0,
            first_failure: None,
        }
    }

    /// Takes the file at `abs` (the matter's `path`, the very path writeback's checks just
    /// verified) out of the folder: into the trash, or, when the trash cannot take it, deleted
    /// for good once the history is known to hold it ([`saved`]). A file the history does not
    /// hold stays, and the error says so. `NotFound` when nothing is there.
    pub(crate) fn remove(&mut self, core: &Core, abs: &Path, path: &MatterPath) -> io::Result<()> {
        let trash = self.trash;
        let mut trashed = false;
        let mut failure = None;
        write::removing(abs, |abs| match trash.trash(abs) {
            Ok(_) => {
                trashed = true;
                Ok(())
            }
            Err(TrashError::Failed(err)) if err.kind() == io::ErrorKind::NotFound => Err(err),
            Err(TrashError::Unsupported) => write::delete_file(abs),
            Err(err) => {
                if !saved(core, path) {
                    // The file stays. The trash's own error when it has one, so a file another
                    // program holds still reads as held.
                    return Err(match err {
                        TrashError::Failed(err) => err,
                        TrashError::Unsupported | TrashError::NoTrash => io::Error::other(
                            "the trash refused a file the history does not hold; it stays",
                        ),
                    });
                }
                failure = Some(err.to_string());
                write::delete_file(abs)
            }
        })?;
        if trashed {
            self.removals.trashed += 1;
        } else {
            self.removals.deleted += 1;
        }
        if let Some(failure) = failure {
            self.fell_back += 1;
            self.first_failure.get_or_insert(failure);
        }
        Ok(())
    }
}

impl Drop for Removing<'_> {
    fn drop(&mut self) {
        if self.fell_back > 0 {
            tracing::warn!(
                files = self.fell_back,
                error = self.first_failure.as_deref().unwrap_or_default(),
                "files deleted without the trash, kept in the history"
            );
        }
    }
}

/// Whether the history holds `path` as the folder has it now: the cache reads it as a blob
/// (writeback just checked the file's stat against that entry), and the tip's tree has that
/// blob at `path`, so the save before the writeback committed it. A failed read counts as not.
fn saved(core: &Core, path: &MatterPath) -> bool {
    let Some(EntryState::Read(blob)) = core.cache.get(path.as_str()).map(|entry| entry.state)
    else {
        return false;
    };
    let Some((_, tree)) = core.timeline.tip() else {
        return false;
    };
    matches!(core.history.file_at(tree, path.as_str()), Ok(Some(at)) if at == blob)
}
