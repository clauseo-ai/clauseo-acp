//! The folder walk: every regular file under a matter folder that the exclusion list keeps, with
//! its stat and whether it is cloud-only or over the size cap. Never follows a symlink, never
//! descends into `.git` or `.clauseo`, never opens a file. [`classify`] is the one rule for
//! which files the crate never reads or writes; undo applies it too.

use std::fs::{self, DirEntry, Metadata};
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::MatterPath;

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
/// denied, say) is listed in [`Listing::unread`] with a warning; the root not reading is an
/// error. Returns `Ok(None)` once `cancel` is set (checked before each folder and each entry).
pub(crate) fn walk(
    root: &Path,
    probe: &dyn PlaceholderProbe,
    cap: u64,
    cancel: Option<&AtomicBool>,
    progress: &mut dyn FnMut(&Found),
) -> Result<Option<Listing>, Error> {
    let mut walker = Walker {
        probe,
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
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if relative.is_empty() => return Err(Error::io("read folder", &dir, err)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => continue,
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
                unread.push(relative);
                continue;
            }
        };
        for entry in entries {
            if walker.cancelled() {
                return Ok(None);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) if relative.is_empty() => {
                    return Err(Error::io("read folder", &dir, err));
                }
                Err(err) => {
                    tracing::warn!(path = %dir.display(), error = %err, "stopped reading a folder");
                    unread.push(relative);
                    break;
                }
            };
            if let Some(sub) = walker.entry(&entry, &relative)? {
                pending.push(sub);
            }
        }
    }
    let mut files = walker.found;
    files.sort_unstable_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    unread.sort_unstable();
    Ok(Some(Listing { files, unread }))
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
    /// file, or returns a folder to read.
    fn entry(
        &mut self,
        entry: &DirEntry,
        parent: &str,
    ) -> Result<Option<(std::path::PathBuf, String)>, Error> {
        let os_name = entry.file_name();
        let path = entry.path();
        let Some(name) = os_name.to_str() else {
            tracing::warn!(path = %path.display(), "skipped a name that is not UTF-8");
            return Ok(None);
        };
        if rules::is_excluded_any(name) {
            return Ok(None);
        }
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", &path, err)),
        };
        let is_dir = file_type.is_dir();
        if !is_dir && (!file_type.is_file() || rules::is_excluded_file(name)) {
            return Ok(None);
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
                return Ok(None);
            }
        };
        if is_dir {
            return Ok(Some((path, relative)));
        }
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", &path, err)),
        };
        // The entry may have been replaced by something else since it was listed.
        let Some(kind) = classify(self.probe, &path, &metadata, self.cap) else {
            return Ok(None);
        };
        let found = Found {
            path: matter_path,
            stat: Stat::from_metadata(&metadata),
            len: metadata.len(),
            kind,
        };
        (self.progress)(&found);
        self.found.push(found);
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
