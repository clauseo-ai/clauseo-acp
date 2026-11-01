//! The folder walk: every regular file under a matter folder that the exclusion list keeps, with
//! its stat and whether it is cloud-only or over the size cap. Never follows a symlink, never
//! descends into `.git` or `.clauseo`, never opens a file. [`classify`] is the one rule for
//! which files the crate never reads or writes; undo applies it too.

use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::MatterPath;

use crate::no_download::{NoDownload, not_downloaded};
use crate::stat::Stat;
use crate::{Error, PlaceholderProbe, rules};

/// One regular file the walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Found {
    /// The file, relative to the walked folder.
    pub(crate) path: MatterPath,
    /// Its stat, taken without following a symlink.
    pub(crate) stat: Stat,
    /// Its size in bytes, untruncated.
    pub(crate) len: u64,
    /// What the walk made of it.
    pub(crate) kind: FoundKind,
}

/// What kind of file the walk found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FoundKind {
    /// A file to version.
    File,
    /// A cloud-only placeholder: never opened.
    CloudOnly,
    /// Larger than the size cap: never opened.
    OverCap,
}

/// What a walk found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listing {
    /// The files, sorted by path bytes.
    pub(crate) files: Vec<Found>,
    /// The subfolders that could not be read (relative to the root, sorted): what is under them
    /// is unknown, so a checkpoint keeps their entries as they were.
    pub(crate) unread: Vec<String>,
}

/// What the crate makes of the entry `path` with `metadata`, taken without following a
/// symlink: `None` when it is not a regular file (never read, never written), else
/// [`FoundKind::CloudOnly`] if `probe` says so, else [`FoundKind::OverCap`] if larger than
/// `cap` ([`crate::Limits::effective_cap`]), else [`FoundKind::File`].
pub(crate) fn classify(
    probe: &dyn PlaceholderProbe,
    path: &Path,
    metadata: &Metadata,
    cap: u64,
) -> Option<FoundKind> {
    if !metadata.is_file() {
        None
    } else if probe.is_cloud_only(path, metadata) {
        Some(FoundKind::CloudOnly)
    } else if metadata.len() > cap {
        Some(FoundKind::OverCap)
    } else {
        Some(FoundKind::File)
    }
}

/// Walks `root`: every regular file under it whose name and folders the exclusion list keeps,
/// sorted by path bytes, each as [`classify`] makes it with `cap`
/// ([`crate::Limits::effective_cap`]). `progress` is called once per file found. Symlinks and
/// other non-regular entries are skipped and never descended; so are names that are not UTF-8
/// or do not make a [`MatterPath`] (with a warning). An entry gone before its stat is skipped,
/// and so is a subfolder gone before it is read. A subfolder that does not read (permission
/// denied, say, or not downloaded) is listed in [`Listing::unread`], with a warning unless it
/// is not downloaded; so is one that lists but cannot be entered (an entry's stat is denied, as
/// for mode `0o444`), with the same warning and nothing of it kept from the listing. The root
/// not reading, or not entering, is an error. Returns `Ok(None)` once `cancel` is
/// set (checked before each folder and each entry). `guard`'s probe tells cloud-only files;
/// the guard keeps a listing from downloading a folder.
pub(crate) fn walk(
    root: &Path,
    guard: &NoDownload<'_>,
    cap: u64,
    cancel: Option<&AtomicBool>,
    progress: &mut dyn FnMut(&Found),
) -> Result<Option<Listing>, Error> {
    let mut walker = Walker {
        probe: guard.probe(),
        cap,
        cancel,
        progress,
        found: Vec::new(),
    };
    let mut unread = Vec::new();
    // Folders still to read: absolute path and path relative to `root` ("" for the root).
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, relative)) = pending.pop() {
        if walker.cancelled() {
            return Ok(None);
        }
        #[cfg(test)]
        let listed = crate::no_download::fault::read_dir(&dir).and_then(|()| fs::read_dir(&dir));
        #[cfg(not(test))]
        let listed = fs::read_dir(&dir);
        let entries = match listed {
            Ok(entries) => entries,
            Err(err) if relative.is_empty() => return Err(Error::io("read folder", &dir, err)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) if not_downloaded(&err) => {
                tracing::info!(path = %dir.display(), "kept a folder that is not downloaded as it was");
                unread.push(relative);
                continue;
            }
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
                unread.push(relative);
                continue;
            }
        };
        // Subfolders found in this folder are pushed after this mark, so a folder that turns
        // out not to enter can drop them again.
        let mark = pending.len();
        let found = walker.found.len();
        for entry in entries {
            if walker.cancelled() {
                return Ok(None);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) if relative.is_empty() => {
                    return Err(Error::io("read folder", &dir, err));
                }
                Err(err) if not_downloaded(&err) => {
                    tracing::info!(path = %dir.display(), "kept a folder that is not downloaded as it was");
                    unread.push(relative);
                    break;
                }
                Err(err) => {
                    tracing::warn!(path = %dir.display(), error = %err, "stopped reading a folder");
                    unread.push(relative);
                    break;
                }
            };
            match walker.entry(&entry, &relative)? {
                Entry::Skip => {}
                Entry::Folder(path, sub) => pending.push((path, sub)),
                Entry::Denied(err) if relative.is_empty() => {
                    return Err(Error::io("stat", &entry.path(), err));
                }
                Entry::Denied(err) => {
                    // Lists but cannot be entered: kept as unread, the same as a folder that
                    // does not list, so one such folder does not stop the checkpoint.
                    tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
                    pending.truncate(mark);
                    walker.found.truncate(found);
                    unread.push(relative);
                    break;
                }
            }
        }
    }
    let mut files = walker.found;
    files.sort_unstable_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    unread.sort_unstable();
    Ok(Some(Listing { files, unread }))
}

/// What [`Walker::entry`] made of one entry.
enum Entry {
    /// Recorded as a file, or left out.
    Skip,
    /// A folder to read: its absolute path and its path relative to the root.
    Folder(std::path::PathBuf, String),
    /// Its stat was denied: the folder it is in lists but cannot be entered.
    Denied(io::Error),
}

struct Walker<'a> {
    probe: &'a dyn PlaceholderProbe,
    cap: u64,
    cancel: Option<&'a AtomicBool>,
    progress: &'a mut dyn FnMut(&Found),
    found: Vec<Found>,
}

impl Walker<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Handles one entry of a folder whose path relative to the root is `parent`: records a
    /// file, returns a folder to read, or says the entry's stat was denied.
    fn entry(&mut self, entry: &DirEntry, parent: &str) -> Result<Entry, Error> {
        let os_name = entry.file_name();
        let path = entry.path();
        let Some(name) = os_name.to_str() else {
            tracing::warn!(path = %path.display(), "skipped a name that is not UTF-8");
            return Ok(Entry::Skip);
        };
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Entry::Skip),
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                return Ok(Entry::Denied(err));
            }
            Err(err) => return Err(Error::io("stat", &path, err)),
        };
        let is_dir = file_type.is_dir();
        if rules::is_excluded(name, is_dir) || (!is_dir && !file_type.is_file()) {
            return Ok(Entry::Skip);
        }
        let relative = if parent.is_empty() {
            name.to_owned()
        } else {
            format!("{parent}/{name}")
        };
        let matter_path = match MatterPath::new(&relative) {
            Ok(matter_path) => matter_path,
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "skipped a path");
                return Ok(Entry::Skip);
            }
        };
        if is_dir {
            return Ok(Entry::Folder(path, relative));
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Entry::Skip),
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                return Ok(Entry::Denied(err));
            }
            Err(err) => return Err(Error::io("stat", &path, err)),
        };
        // The entry may have been replaced by something else since it was listed.
        let Some(kind) = classify(self.probe, &path, &metadata, self.cap) else {
            return Ok(Entry::Skip);
        };
        let found = Found {
            path: matter_path,
            stat: Stat::from_metadata(&metadata),
            len: metadata.len(),
            kind,
        };
        (self.progress)(&found);
        self.found.push(found);
        Ok(Entry::Skip)
    }
}

#[cfg(test)]
mod tests;
