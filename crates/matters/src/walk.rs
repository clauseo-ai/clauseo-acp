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
/// not reading, or not entering, is an error. Returns `Ok(None)` once `cancel` is set
/// (checked before each folder, each entry and each stat). `guard`'s probe tells
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
        if !walker.read_folder(&dir, &relative, &mut items)? {
            return Ok(None);
        }
        // What this folder adds goes after these marks, so a folder that turns out not to
        // enter can drop it again.
        let (found_mark, pending_mark) = (found.len(), pending.len());
        for item in items.drain(..) {
            match item {
                Item::File(entry, path) => {
                    if walker.cancelled() {
                        return Ok(None);
                    }
                    match walker.stat(&entry, path) {
                        Stated::Found(file) => {
                            progress(&file);
                            found.push(file);
                        }
                        Stated::Gone => {}
                        Stated::Denied(err) => {
                            walker.unenterable(&dir, &relative, entry.path(), err)?;
                            found.truncate(found_mark);
                            pending.truncate(pending_mark);
                            break;
                        }
                        Stated::Failed(err) => return Err(err),
                    }
                }
                Item::Stated(file) => {
                    progress(&file);
                    found.push(file);
                }
                Item::Folder(dir, relative) => pending.push((dir, relative)),
            }
        }
        items.clear();
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
/// a file that does, but for each folder's first file (a folder is sorted by its entries'
/// names and types, which reading it gives). A subfolder that does not read, or stops
/// reading, counts as one entry in its place (as `<folder>/`), and goes in
/// [`Listing::unread`]: so `files` and `unread` together hold at most `limit + 1` entries,
/// the first in that order. A folder whose every stat is denied
/// (it lists but cannot be entered, mode `0o444`) is unread as in [`walk`]: the stat of its
/// first file is taken when the folder is read (it can sort after the cut when the folder's
/// subfolders sort first), so none of it is listed even when the cut falls inside it. A folder
/// with one denied file among others that stat (a security module, a FUSE mount) is unread
/// once the walk reaches that file; files of it listed before the cut stay listed.
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
            Item::File(entry, path) => match walker.stat(&entry, path.clone()) {
                Stated::Found(file) => found.push(file),
                Stated::Gone => {}
                // Denied after its folder's first stat passed: drop all of that folder.
                Stated::Denied(err) => {
                    let (folder, _) = path.as_str().rsplit_once('/').unwrap_or(("", ""));
                    let dir = entry
                        .path()
                        .parent()
                        .map(Path::to_path_buf)
                        .unwrap_or_default();
                    walker.unenterable(&dir, folder, entry.path(), err)?;
                    let prefix = format!("{folder}/");
                    let under = |path: &str| path.starts_with(&prefix);
                    while found
                        .last()
                        .is_some_and(|file: &Found| under(file.path.as_str()))
                    {
                        found.pop();
                    }
                    walker.unread.retain(|dir| !under(dir));
                    while pending.last().is_some_and(|item| under(item.path())) {
                        pending.pop();
                    }
                }
                Stated::Failed(err) => return Err(err),
            },
            Item::Stated(file) => found.push(file),
            Item::Folder(dir, relative) => {
                // No cancel flag, so the read always finishes.
                walker.read_folder(&dir, &relative, &mut items)?;
                items.sort_unstable_by(|a, b| a.sort_key().cmp(b.sort_key()));
                if !walker.first_stat(&dir, &relative, &mut items)? {
                    items.clear();
                }
                pending.extend(items.drain(..).rev());
            }
        }
        if found.len() + walker.unread.len() > limit {
            break;
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
    /// A file whose stat is taken.
    Stated(Found),
    /// A subfolder to read: absolute path and path relative to the root.
    Folder(std::path::PathBuf, String),
}

/// What [`Walker::stat`] made of a file.
enum Stated {
    /// A file the walk lists.
    Found(Found),
    /// Gone, or no longer a regular file: left out.
    Gone,
    /// Its stat was denied: the folder it is in lists but cannot be entered.
    Denied(io::Error),
    /// Any other failure, which stops the walk.
    Failed(Error),
}

impl Item {
    /// Its path relative to the root.
    fn path(&self) -> &str {
        match self {
            Self::File(_, path) => path.as_str(),
            Self::Stated(file) => file.path.as_str(),
            Self::Folder(_, relative) => relative,
        }
    }

    /// The bytes it sorts by: a file's path, or a folder's with `/` after it, which sorts
    /// every path under the folder where the folder's own name would.
    fn sort_key(&self) -> impl Iterator<Item = u8> + '_ {
        let (path, slash) = match self {
            Self::File(_, path) => (path.as_str(), None),
            Self::Stated(file) => (file.path.as_str(), None),
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
    /// stays in `items`), and so does one with an entry whose type is denied, with nothing of it
    /// in `items`; the root not reading or entering is an error.
    fn read_folder(
        &mut self,
        dir: &Path,
        relative: &str,
        items: &mut Vec<Item>,
    ) -> Result<bool, Error> {
        if self.cancelled() {
            return Ok(false);
        }
        let mark = items.len();
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
                self.unread.push(relative.to_owned());
                return Ok(true);
            }
            Err(err) => {
                tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
                self.unread.push(relative.to_owned());
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
                    self.unread.push(relative.to_owned());
                    break;
                }
                Err(err) => {
                    tracing::warn!(path = %dir.display(), error = %err, "stopped reading a folder");
                    self.unread.push(relative.to_owned());
                    break;
                }
            };
            match entry_item(entry, relative)? {
                Entry::Skip => {}
                Entry::Item(item) => items.push(item),
                Entry::Denied(path, err) => {
                    self.unenterable(dir, relative, path, err)?;
                    items.truncate(mark);
                    break;
                }
            }
        }
        Ok(true)
    }

    /// The folder `dir` (`relative` to the root) lists but cannot be entered: the stat of
    /// `path` in it failed with `err`. Kept as unread, as a folder that does not list, so one
    /// such folder does not stop a checkpoint; the root is an error.
    fn unenterable(
        &mut self,
        dir: &Path,
        relative: &str,
        path: std::path::PathBuf,
        err: io::Error,
    ) -> Result<(), Error> {
        if relative.is_empty() {
            return Err(Error::io("stat", path, err));
        }
        tracing::warn!(path = %dir.display(), error = %err, "skipped a folder that does not read");
        // A folder that stopped reading part way, or whose subfolder was dropped with it, may
        // be listed already.
        if !self.unread.iter().any(|dir| dir == relative) {
            self.unread.push(relative.to_owned());
        }
        Ok(())
    }

    /// Takes the stat of the first file of `items` (one folder's, sorted), which says whether
    /// the folder `dir` can be entered: `Ok(false)` when it cannot (now in `unread`). A file
    /// gone is dropped and the next one tried.
    fn first_stat(
        &mut self,
        dir: &Path,
        relative: &str,
        items: &mut Vec<Item>,
    ) -> Result<bool, Error> {
        let mut at = 0;
        while let Some(item) = items.get(at) {
            let Item::File(entry, path) = item else {
                at += 1;
                continue;
            };
            match self.stat(entry, path.clone()) {
                Stated::Found(file) => {
                    if let Some(slot) = items.get_mut(at) {
                        *slot = Item::Stated(file);
                    }
                    return Ok(true);
                }
                Stated::Gone => {
                    items.remove(at);
                }
                Stated::Denied(err) => {
                    let path = entry.path();
                    self.unenterable(dir, relative, path, err)?;
                    return Ok(false);
                }
                Stated::Failed(err) => return Err(err),
            }
        }
        Ok(true)
    }

    /// The file `entry` at `path` with its stat, as [`classify`] makes it.
    fn stat(&self, entry: &DirEntry, path: MatterPath) -> Stated {
        let absolute = entry.path();
        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Stated::Gone,
            Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
                return Stated::Denied(err);
            }
            Err(err) => return Stated::Failed(Error::io("stat", &absolute, err)),
        };
        // The entry may have been replaced by something else since it was listed.
        let Some(kind) = classify(self.probe, &absolute, &metadata, self.cap) else {
            return Stated::Gone;
        };
        Stated::Found(Found {
            path,
            stat: Stat::from_metadata(&metadata),
            len: metadata.len(),
            kind,
        })
    }
}

/// What [`entry_item`] made of one entry.
enum Entry {
    /// Left out.
    Skip,
    /// A file or a folder to read.
    Item(Item),
    /// Its type was denied (the entry's path and the error): its folder cannot be entered.
    Denied(std::path::PathBuf, io::Error),
}

/// Makes one entry of a folder whose path relative to the root is `parent` into a file or
/// a folder to read.
fn entry_item(entry: DirEntry, parent: &str) -> Result<Entry, Error> {
    let os_name = entry.file_name();
    let Some(name) = os_name.to_str() else {
        tracing::warn!(path = %entry.path().display(), "skipped a name that is not UTF-8");
        return Ok(Entry::Skip);
    };
    let file_type = match entry.file_type() {
        Ok(file_type) => file_type,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Entry::Skip),
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            return Ok(Entry::Denied(entry.path(), err));
        }
        Err(err) => return Err(Error::io("stat", entry.path(), err)),
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
            tracing::warn!(path = %entry.path().display(), error = %err, "skipped a path");
            return Ok(Entry::Skip);
        }
    };
    if is_dir {
        return Ok(Entry::Item(Item::Folder(entry.path(), relative)));
    }
    Ok(Entry::Item(Item::File(entry, matter_path)))
}

#[cfg(test)]
mod tests;
