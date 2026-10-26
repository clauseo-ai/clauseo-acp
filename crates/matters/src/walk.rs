//! The folder walk: every regular file under a matter folder that the exclusion list keeps, with
//! its stat and whether it is cloud-only or over the size cap. Never follows a symlink, never
//! descends into `.git` or `.clauseo`, never opens a file.

use std::fs::{self, DirEntry};
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

/// Walks `root`: every regular file under it whose name and folders the exclusion list keeps,
/// sorted by path bytes. A file is [`FoundKind::CloudOnly`] if `probe` says so, else
/// [`FoundKind::OverCap`] if larger than `file_cap` (at most `u32::MAX`). `progress` is called
/// once per file found. Symlinks and other non-regular entries are skipped and never
/// descended; so are names that are not UTF-8 or do not make a [`MatterPath`] (with a warning).
/// An entry gone before its stat is skipped. Returns `Ok(None)` once `cancel` is set (checked
/// before each folder and each entry).
pub(crate) fn walk(
    root: &Path,
    probe: &dyn PlaceholderProbe,
    file_cap: u64,
    cancel: Option<&AtomicBool>,
    progress: &mut dyn FnMut(&Found),
) -> Result<Option<Vec<Found>>, Error> {
    let mut walker = Walker {
        probe,
        cap: file_cap.min(u64::from(u32::MAX)),
        cancel,
        progress,
        found: Vec::new(),
    };
    // Folders still to read: absolute path and path relative to `root` ("" for the root).
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, relative)) = pending.pop() {
        if walker.cancelled() {
            return Ok(None);
        }
        let entries = fs::read_dir(&dir).map_err(|err| Error::io("read folder", &dir, err))?;
        for entry in entries {
            if walker.cancelled() {
                return Ok(None);
            }
            let entry = entry.map_err(|err| Error::io("read folder", &dir, err))?;
            if let Some(sub) = walker.entry(&entry, &relative)? {
                pending.push(sub);
            }
        }
    }
    let mut found = walker.found;
    found.sort_unstable_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    Ok(Some(found))
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
        if !metadata.is_file() {
            return Ok(None);
        }
        let len = metadata.len();
        let kind = if self.probe.is_cloud_only(&path, &metadata) {
            FoundKind::CloudOnly
        } else if len > self.cap {
            FoundKind::OverCap
        } else {
            FoundKind::File
        };
        let found = Found {
            path: matter_path,
            stat: Stat::from_metadata(&metadata),
            len,
            kind,
        };
        (self.progress)(&found);
        self.found.push(found);
        Ok(None)
    }
}

#[cfg(test)]
mod tests;
