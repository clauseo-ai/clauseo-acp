//! Writing history back into the matter folder, shared by undo and restore: which paths may be
//! written, folder listings that match names byte for byte, and the one sequence that stages
//! every write, checks the targets again, and renames.

use std::collections::{HashMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::fs::Metadata;
use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterPath;

use crate::checkpoint::Core;
use crate::history::{HistoryStore, Oid};
use crate::stat::Stat;
use crate::types::Settings;
use crate::walk::{self, FoundKind};
use crate::{Error, fsync, write};

/// One file operation on the folder.
#[derive(Debug, Clone)]
pub(crate) enum Op {
    /// Write this blob to the path.
    Write(MatterPath, Oid),
    /// Delete the path.
    Delete(MatterPath),
}

impl Op {
    pub(crate) const fn path(&self) -> &MatterPath {
        match self {
            Self::Write(path, _) | Self::Delete(path) => path,
        }
    }
}

/// Folder listings, each read once, to tell whether a path names its entries by their exact
/// bytes. On a case- or normalisation-insensitive file system (APFS by default, NTFS)
/// `symlink_metadata` of `memo.txt` also finds `Memo.txt`, which the history counts as
/// another file.
#[derive(Default)]
pub(crate) struct Names {
    listed: HashMap<PathBuf, HashSet<OsString>>,
}

impl Names {
    /// The metadata of `folder`/`path`, taken without following a symlink, when every part of
    /// `path` is an entry of exactly that name; `None` when something is missing or only an
    /// entry spelled otherwise is there.
    pub(crate) fn metadata(
        &mut self,
        folder: &Path,
        path: &MatterPath,
    ) -> Result<Option<Metadata>, Error> {
        let abs = folder.join(path.as_str());
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", abs, err)),
        };
        let mut dir = folder.to_owned();
        for part in path.as_str().split('/') {
            if !self.holds(&dir, part)? {
                return Ok(None);
            }
            dir.push(part);
        }
        Ok(Some(metadata))
    }

    /// The metadata of the entry the file system finds at `folder`/`path` when the folder does
    /// not hold `path` by its exact bytes: a file spelled otherwise (another case, another
    /// Unicode normalisation) on a case- or normalisation-insensitive file system. Writing
    /// `path` would replace it. `None` when nothing is there or `path` is there exactly. A
    /// folder on the way spelled otherwise, with no file under the name, is not one: a write
    /// then makes a new file in that folder and replaces nothing.
    pub(crate) fn other_spelling(
        &mut self,
        folder: &Path,
        path: &MatterPath,
    ) -> Result<Option<Metadata>, Error> {
        let abs = folder.join(path.as_str());
        let metadata = match std::fs::symlink_metadata(&abs) {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", abs, err)),
        };
        if self.metadata(folder, path)?.is_some() {
            return Ok(None);
        }
        Ok(Some(metadata))
    }

    /// Whether the folder `dir` holds an entry named exactly `name`.
    fn holds(&mut self, dir: &Path, name: &str) -> Result<bool, Error> {
        if !self.listed.contains_key(dir) {
            let mut names = HashSet::new();
            let entries = match std::fs::read_dir(dir) {
                Ok(entries) => Some(entries),
                Err(err) if err.kind() == io::ErrorKind::NotFound => None,
                Err(err) => return Err(Error::io("read folder", dir, err)),
            };
            for entry in entries.into_iter().flatten() {
                let entry = entry.map_err(|err| Error::io("read folder", dir, err))?;
                names.insert(entry.file_name());
            }
            self.listed.insert(dir.to_owned(), names);
        }
        Ok(self
            .listed
            .get(dir)
            .is_some_and(|names| names.contains(OsStr::new(name))))
    }
}

/// The paths of `ops` that would touch a file the folder holds only spelled otherwise (another
/// case or Unicode normalisation, on APFS or NTFS), sorted: a write would replace that file and
/// a delete would remove it, though the history counts it as another file. A write is let
/// through when the file it finds is one of these `ops` deletes by its exact name, as when
/// undoing a case-only rename (`brief.txt` back to `Brief.txt`).
pub(crate) fn respelled(
    names: &mut Names,
    folder: &Path,
    ops: &[&Op],
) -> Result<Vec<MatterPath>, Error> {
    let mut deleted: Vec<(PathBuf, Metadata)> = Vec::new();
    for op in ops {
        if let Op::Delete(path) = op
            && let Some(metadata) = names.metadata(folder, path)?
        {
            deleted.push((folder.join(path.as_str()), metadata));
        }
    }
    let mut clashes = Vec::new();
    for op in ops {
        let path = op.path();
        let Some(other) = names.other_spelling(folder, path)? else {
            continue;
        };
        let abs = folder.join(path.as_str());
        let ours = matches!(op, Op::Write(..))
            && deleted
                .iter()
                .any(|(gone, metadata)| same_entry(gone, metadata, &abs, &other));
        if !ours {
            clashes.push(path.clone());
        }
    }
    clashes.sort();
    clashes.dedup();
    Ok(clashes)
}

/// Whether the entries found at `a` and `b` are one file: same device and inode on Unix;
/// elsewhere, where std gives no file id, paths equal but for case.
fn same_entry(a: &Path, a_meta: &Metadata, b: &Path, b_meta: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = (a, b);
        a_meta.dev() == b_meta.dev() && a_meta.ino() == b_meta.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (a_meta, b_meta);
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

/// Whether the entry at `abs` is one the crate never reads or writes, by the walk's rule:
/// not a regular file, cloud-only, or over the cap.
pub(crate) fn skipped(settings: &Settings, abs: &Path, metadata: &Metadata) -> bool {
    let cap = settings.limits.effective_cap();
    walk::classify(settings.probe.as_ref(), abs, metadata, cap) != Some(FoundKind::File)
}

/// Whether `path` may be written or deleted: no folder on the way is a symlink (or a junction,
/// or not a folder), the entry itself is a regular file or absent, and it is not cloud-only or
/// over the cap. On Windows, no part of the path is a device name.
pub(crate) fn writable(
    folder: &Path,
    settings: &Settings,
    path: &MatterPath,
) -> Result<bool, Error> {
    let mut at = folder.to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        #[cfg(windows)]
        if clauseo_protocol::is_windows_reserved_name(part) {
            return Ok(false);
        }
        at.push(part);
        let metadata = match std::fs::symlink_metadata(&at) {
            Ok(metadata) => metadata,
            // Nothing there: the rest of the path is made fresh.
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(err) => return Err(Error::io("stat", &at, err)),
        };
        if parts.peek().is_some() {
            if !metadata.is_dir() {
                return Ok(false);
            }
        } else {
            return Ok(!skipped(settings, &at, &metadata));
        }
    }
    Ok(true)
}

/// Carries out `ops` so that a failure while writing changes nothing, and a deletion never
/// removes a file just written:
///
/// 1. every write goes to a synced temp file beside its target (making missing folders);
/// 2. every target is stat'd again against the cache the caller's `outside-edits` checkpoint
///    just wrote, and checked again for a file spelled otherwise ([`respelled`]): one that
///    changed or appeared since fails with `matter.file_locked`, the temp files are removed,
///    and so are the folders step 1 made;
/// 3. the deletions are made;
/// 4. the temp files are renamed over their targets, then each folder that got one is synced
///    once (one sync per folder, not per file).
///
/// A crash in step 1 or 2 leaves the folder's files as they were, with `.clauseo-<n>.tmp` files
/// beside them at worst (excluded from every checkpoint); a crash in step 3 or 4 leaves the
/// folder partly written, each file old or new and never torn (each was synced before its
/// rename; a rename whose folder was not synced yet may be lost, leaving the old file),
/// recorded by nothing until the next checkpoint. `apply` returns only after every folder is
/// synced, so the checkpoint the caller takes next never records a rename that could be lost.
///
/// Deleting before renaming matters on a case- or normalisation-insensitive file system: undoing
/// a rename of `Brief.txt` to `brief.txt` deletes `brief.txt`, which is the same file as
/// `Brief.txt` there; deleting after the rename would remove what was just put back.
pub(crate) fn apply(core: &Core, folder: &Path, ops: &[&Op]) -> Result<(), Error> {
    let mut made = Vec::new();
    let staged = match stage_all(core, folder, ops, &mut made) {
        Ok(staged) => staged,
        Err(err) => {
            // The temp files went with `stage_all`; the folders it made are empty again.
            for dir in made.iter().rev() {
                if let Err(err) = write::remove_dir(dir) {
                    tracing::warn!(path = %dir.display(), error = %err, "folder left behind");
                }
            }
            return Err(err);
        }
    };
    for op in ops {
        if let Op::Delete(path) = op {
            let abs = folder.join(path.as_str());
            match write::remove_file(&abs) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
                Err(err) => return Err(Error::io("delete", abs, err)),
            }
            let dir = abs.parent().unwrap_or(folder);
            fsync::sync_dir(dir).map_err(|err| Error::io("sync", dir, err))?;
        }
    }
    // Every rename, then each folder synced once: the renames are durable before the
    // checkpoint that follows records them.
    let mut dirs = std::collections::BTreeSet::new();
    for file in staged {
        dirs.insert(file.rename()?);
    }
    for dir in dirs {
        fsync::sync_dir(&dir).map_err(|err| Error::io("sync", &dir, err))?;
    }
    // macOS: the syncs above reach the drive but not past its cache, and the checkpoint's
    // `F_FULLFSYNC` is on the matters root, which may be another drive.
    #[cfg(target_os = "macos")]
    {
        let dir = std::fs::File::open(folder).map_err(|err| Error::io("open", folder, err))?;
        fsync::full_flush(&dir).map_err(|err| Error::io("sync", folder, err))?;
    }
    Ok(())
}

/// Steps 1 and 2 of [`apply`]: the staged writes, or an error with every temp file removed;
/// `made` gets each folder made on the way, outermost first.
fn stage_all(
    core: &Core,
    folder: &Path,
    ops: &[&Op],
    made: &mut Vec<PathBuf>,
) -> Result<Vec<write::Staged>, Error> {
    let mut staged = Vec::new();
    for op in ops {
        if let Op::Write(path, blob) = op {
            let abs = make_parents(folder, path, made)?;
            let file = write::stage(&abs, &mut |file| {
                core.history.read_blob(*blob, file).map(drop)
            })?;
            staged.push(file);
        }
    }
    let moved = moved_since(core, folder, ops)?;
    if !moved.is_empty() {
        return Err(Error::FileLocked { paths: moved });
    }
    Ok(staged)
}

/// The targets of `ops` whose stat differs from the cache's entry (or that appeared or went
/// since), and those a file spelled otherwise now stands for ([`respelled`]), sorted:
/// something wrote to them after the cache was written. No checkpoint has seen a file spelled
/// otherwise that appeared after the walk, so replacing it would lose it for good.
fn moved_since(core: &Core, folder: &Path, ops: &[&Op]) -> Result<Vec<MatterPath>, Error> {
    let mut names = Names::default();
    let mut moved = Vec::new();
    for op in ops {
        let path = op.path();
        let now = names
            .metadata(folder, path)?
            .map(|metadata| Stat::from_metadata(&metadata));
        let then = core.cache.get(path.as_str()).map(|entry| entry.stat);
        if now != then {
            moved.push(path.clone());
        }
    }
    moved.extend(respelled(&mut names, folder, ops)?);
    moved.sort();
    moved.dedup();
    Ok(moved)
}

/// `folder`/`path`, after making each missing folder on the way (and syncing the folder
/// that holds it), each pushed to `made`. [`writable`] already refused a path through a
/// symlink.
fn make_parents(
    folder: &Path,
    path: &MatterPath,
    made: &mut Vec<PathBuf>,
) -> Result<PathBuf, Error> {
    let mut at = folder.to_owned();
    let mut parts = path.as_str().split('/').peekable();
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            at.push(part);
            break;
        }
        let parent = at.clone();
        at.push(part);
        match write::create_dir(&at) {
            Ok(()) => {
                made.push(at.clone());
                fsync::sync_dir(&parent).map_err(|err| Error::io("sync", &parent, err))?;
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) => return Err(Error::io("create", &at, err)),
        }
    }
    Ok(at)
}
