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
/// is not downloaded; the root not reading is an error. Returns `Ok(None)` once `cancel` is
/// set (checked before each folder, each entry and each stat). `guard`'s probe tells
/// cloud-only files; the guard keeps a listing from downloading a folder.
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
        unread: Vec::new(),
    };
    let mut found = Vec::new();
    let mut items = Vec::new();
    // Folders still to read: absolute path and path relative to `root` ("" for the root).
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, relative)) = pending.pop() {
        if !walker.read_folder(&dir, relative, &mut items)? {
            return Ok(None);
        }
        for item in items.drain(..) {
            match item {
                Item::File(entry, path) => {
                    if walker.cancelled() {
                        return Ok(None);
                    }
                    if let Some(file) = walker.stat(&entry, path)? {
                        progress(&file);
                        found.push(file);
                    }
                }
                Item::Folder(dir, relative) => pending.push((dir, relative)),
            }
        }
    }
    found.sort_unstable_by(|a, b| a.path.as_str().as_bytes().cmp(b.path.as_str().as_bytes()));
    let mut unread = walker.unread;
    unread.sort_unstable();
    Ok(Some(Listing {
        files: found,
        unread,
    }))
}

/// The first `limit + 1` files [`walk`] would list, in its order, by the same rules: each
/// folder's entries are taken in path-byte order (a folder `a` as `a/`, so `a.txt` comes
/// before what it holds and `a0` after), so the files come out sorted and the walk stops at
/// the file past `limit`, never reading a folder that sorts after it nor taking the stat of
/// a file that does (a folder is sorted by its entries' names and types, which reading it
/// gives). A subfolder that does not read, or stops reading, counts as one entry in its place
/// (as `<folder>/`), and goes in [`Listing::unread`]: so `files` and `unread` together hold
/// at most `limit + 1` entries, the first in that order.
pub(crate) fn walk_first(
    root: &Path,
    guard: &NoDownload<'_>,
    cap: u64,
    limit: usize,
) -> Result<Listing, Error> {
    let mut walker = Walker {
        probe: guard.probe(),
        cap,
        cancel: None,
        unread: Vec::new(),
    };
    let mut found = Vec::new();
    let mut items = Vec::new();
    let mut pending = vec![Item::Folder(root.to_path_buf(), String::new())];
    while let Some(next) = pending.pop() {
        match next {
            Item::File(entry, path) => {
                if let Some(file) = walker.stat(&entry, path)? {
                    found.push(file);
                    if found.len() + walker.unread.len() > limit {
                        break;
                    }
                }
            }
            Item::Folder(dir, relative) => {
                // No cancel flag, so the read always finishes.
                walker.read_folder(&dir, relative, &mut items)?;
                if found.len() + walker.unread.len() > limit {
                    // That folder did not read, and was the entry past the limit.
                    items.clear();
                    break;
                }
                items.sort_unstable_by(|a, b| a.sort_key().cmp(b.sort_key()));
                pending.extend(items.drain(..).rev());
            }
        }
    }
    let mut unread = walker.unread;
    unread.sort_unstable();
    Ok(Listing {
        files: found,
        unread,
    })
}

/// One entry of a folder that the walk keeps.
enum Item {
    /// A regular file whose stat is not taken yet, and its path relative to the root.
    File(DirEntry, MatterPath),
    /// A subfolder to read: absolute path and path relative to the root.
    Folder(std::path::PathBuf, String),
}

impl Item {
    /// The bytes it sorts by: a file's path, or a folder's with `/` after it, which sorts
    /// every path under the folder where the folder's own name would.
    fn sort_key(&self) -> impl Iterator<Item = u8> + '_ {
        let (path, slash) = match self {
            Self::File(_, path) => (path.as_str(), None),
            Self::Folder(_, relative) => (relative.as_str(), Some(b'/')),
        };
        path.bytes().chain(slash)
    }
}

struct Walker<'a> {
    probe: &'a dyn PlaceholderProbe,
    cap: u64,
    cancel: Option<&'a AtomicBool>,
    /// The subfolders that did not read, in the order found.
    unread: Vec<String>,
}

impl Walker<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Reads the folder `dir`, whose path relative to the root is `relative` ("" for the root),
    /// adding its kept entries to `items`. `Ok(false)` once cancelled. A subfolder gone is
    /// skipped; one that does not read, or stops reading, goes in `unread` (what it gave so far
    /// stays in `items`); the root not reading is an error.
    fn read_folder(
        &mut self,
        dir: &Path,
        relative: String,
        items: &mut Vec<Item>,
    ) -> Result<bool, Error> {
        if self.cancelled() {
            return Ok(false);
        }
        #[cfg(test)]
        let listed = crate::no_download::fault::read_dir(dir).and_then(|()| fs::read_dir(dir));
        #[cfg(not(test))]
        let listed = fs::read_dir(dir);
        let entries = match listed {
            Ok(entries) => entries,
            Err(err) if relative.is_empty() => return Err(Error::io("read folder", dir, err)),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(err) if not_downloaded(&err) => {
                tracing::info!(path = %dir.display(), "kept a folder that is not downloaded as it was");
                self.unread.push(relative);
                return Ok(true);
            }
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
                self.unread.push(relative);
                return Ok(true);
            }
        };
        for entry in entries {
            if self.cancelled() {
                return Ok(false);
            }
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) if relative.is_empty() => {
                    return Err(Error::io("read folder", dir, err));
                }
                Err(err) if not_downloaded(&err) => {
                    tracing::info!(path = %dir.display(), "kept a folder that is not downloaded as it was");
                    self.unread.push(relative);
                    break;
                }
                Err(err) => {
                    tracing::warn!(path = %dir.display(), error = %err, "stopped reading a folder");
                    self.unread.push(relative);
                    break;
                }
            };
            if let Some(item) = entry_item(entry, &relative)? {
                items.push(item);
            }
        }
        Ok(true)
    }

    /// The file `entry` at `path` with its stat, as [`classify`] makes it: `None` when it
    /// is gone or no longer a regular file.
    fn stat(&self, entry: &DirEntry, path: MatterPath) -> Result<Option<Found>, Error> {
        let absolute = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(err) => return Err(Error::io("stat", &absolute, err)),
        };
        // The entry may have been replaced by something else since it was listed.
        let Some(kind) = classify(self.probe, &absolute, &metadata, self.cap) else {
            return Ok(None);
        };
        Ok(Some(Found {
            path,
            stat: Stat::from_metadata(&metadata),
            len: metadata.len(),
            kind,
        }))
    }
}

/// Makes one entry of a folder whose path relative to the root is `parent` into a file or
/// a folder to read, or `None` when the walk skips it.
fn entry_item(entry: DirEntry, parent: &str) -> Result<Option<Item>, Error> {
    let os_name = entry.file_name();
    let Some(name) = os_name.to_str() else {
        tracing::warn!(path = %entry.path().display(), "skipped a name that is not UTF-8");
        return Ok(None);
    };
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(Error::io("stat", entry.path(), err)),
    };
    let is_dir = file_type.is_dir();
    if rules::is_excluded(name, is_dir) || (!is_dir && !file_type.is_file()) {
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
            tracing::warn!(path = %entry.path().display(), error = %err, "skipped a path");
            return Ok(None);
        }
    };
    if is_dir {
        return Ok(Some(Item::Folder(entry.path(), relative)));
    }
    Ok(Some(Item::File(entry, matter_path)))
}

#[cfg(test)]
mod tests;
