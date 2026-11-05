//! The set-aside's own scan of the matter tree (S15 `PR3b` round 3, R1 and R2). It reads and
//! never writes, has no cap, and never follows a link: each folder is opened relative to its
//! parent's handle, and a link is recorded, never entered. It doesn't depend on the ACL walk,
//! so a walk that refuses (past its set-aside cap, say) never stops the check.
//!
//! In each folder it visits, it probes each protected name by opening it relative to that
//! folder's handle, as git and Claude would open it: at the root each of
//! [`PROTECTED_NAMES`](crate::PROTECTED_NAMES) and `.git`, at every depth below `.git`,
//! `.claude` and `.mcp.json`. On NTFS the open ignores case as NTFS does, so whatever the volume
//! opens for `.git` counts as `.git`; no name is folded here. Each link it lists (a junction or
//! symbolic link, whatever its name) is recorded by its own file ID.
//!
//! A folder it can't open or list is named in [`Scan::unscanned`]: the scan carries on, what it
//! found is still checked, and the pass says the scan was partial.
//!
//! Names are kept as the folder's listing gives them, text or not (round 4): a name that isn't
//! valid UTF-16 text, a lone surrogate say, is scanned, recorded and compared like any other.
//! Each item also carries the file IDs of the folders it was reached through, so the check can
//! pass over exactly what is inside a folder it set aside.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use super::SetAsideFailure;
use super::files::{FileId, io_failure};
use super::names::Key;
use super::run::Volume;
use crate::PROTECTED_NAMES;

/// Also probed at the root: hooks in a `.git` the agent made would run in the lawyer's own git.
const ALSO: [&str; 1] = [".git"];

/// Probed in every folder below the root: git finds a `.git` at any depth, and Claude loads
/// `.claude` and `.mcp.json` from the folder it works in.
const NESTED: [&str; 3] = [".git", ".claude", ".mcp.json"];

/// Each protected name probed at the matter's root.
fn root_names() -> impl Iterator<Item = &'static str> {
    PROTECTED_NAMES.iter().chain(&ALSO).copied()
}

/// What a folder's listing says a name is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// A folder that isn't a link: the scan goes into it.
    Folder,
    /// A junction, symbolic link or other name surrogate: recorded, never entered.
    Link,
    /// Anything else.
    Other,
}

/// One name a folder's listing gave.
#[derive(Debug, Clone)]
pub(super) struct Listed {
    pub(super) name: OsString,
    pub(super) kind: Kind,
}

/// What is at a protected name or a link now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Now {
    /// Nothing.
    Absent,
    /// The file, folder or link with this ID.
    At(FileId),
    /// Something whose ID the lawyer can't read: its permissions shut the lawyer out.
    Shut,
    /// Its ID couldn't be read.
    Failed(SetAsideFailure),
}

/// Which part of the record an item is kept in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kept {
    /// A protected name at the root: recorded even when absent.
    Root,
    /// A protected name below the root.
    Nested,
    /// A link, in [`Record::links`](super::files::Record::links).
    Link,
}

/// One protected name or link the scan found.
#[derive(Debug, Clone)]
pub(super) struct Item {
    /// Its path below the root, one name per part: its key in the record.
    pub(super) key: Key,
    /// Its path: the root joined with each part.
    pub(super) path: PathBuf,
    /// The folders it is in below the root.
    pub(super) within: Vec<OsString>,
    /// The file IDs of those folders, as the scan opened them: what it was reached through.
    pub(super) through: Vec<FileId>,
    pub(super) now: Now,
    pub(super) kept: Kept,
}

/// What a scan found: each protected name and link, a folder before what is inside it, and
/// each folder it couldn't open or list.
#[derive(Debug, Clone, Default)]
pub(super) struct Scan {
    pub(super) items: Vec<Item>,
    /// The folders it couldn't open or list (the root, when it couldn't open that). When there
    /// is any, the scan was partial.
    pub(super) unscanned: Vec<PathBuf>,
    /// The key of each folder (not a link) the listings it read gave, entered or not: what a
    /// re-probe of one folder's own names knows of the folders in it.
    pub(super) folders: Vec<Key>,
}

/// Scans the matter tree below `root` through `volume`; see the module's docs.
pub(super) fn scan<V: Volume>(volume: &V, root: &Path) -> Scan {
    let mut scan = Scan::default();
    let top = match volume.open_root(root) {
        Ok(top) => top,
        Err(error) => {
            unscanned(&mut scan, root, &error);
            return scan;
        }
    };
    scan_from(
        volume,
        &mut scan,
        (top, root.to_owned(), Vec::new(), Vec::new()),
        true,
    );
    if !scan.unscanned.is_empty() {
        tracing::warn!(
            folders = scan.unscanned.len(),
            "the set-aside couldn't scan every folder in the matter"
        );
    }
    scan
}

/// The scan of one folder, `parts` below `root` (each part opened relative to the one before,
/// never through a link): its own protected names and links, and with `below` everything
/// below it too, as [`scan`] finds them. `None` when a folder on the way is gone. A later
/// launch re-probes the folders the journal says changed this way (S15 `PR3d`).
pub(super) fn scan_part<V: Volume>(
    volume: &V,
    root: &Path,
    parts: &[OsString],
    below: bool,
) -> io::Result<Option<Scan>> {
    let mut folder = volume.open_root(root)?;
    let mut path = root.to_owned();
    let mut through = Vec::new();
    for part in parts {
        match volume.open_folder(&folder, part) {
            Ok(inner) => {
                through.push(volume.folder_id(&inner)?);
                folder = inner;
                path.push(part);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    let mut scan = Scan::default();
    scan_from(
        volume,
        &mut scan,
        (folder, path, parts.to_vec(), through),
        below,
    );
    Ok(Some(scan))
}

/// A folder the scan will visit: it, its path, its parts below the root and the IDs of the
/// folders it was reached through.
type Visit<F> = (F, PathBuf, Vec<OsString>, Vec<FileId>);

/// Scans `start`, and with `below` every folder below it, into `scan`.
fn scan_from<V: Volume>(volume: &V, scan: &mut Scan, start: Visit<V::Folder>, below: bool) {
    let mut stack = vec![start];
    while let Some((folder, path, parts, through)) = stack.pop() {
        let at_root = parts.is_empty();
        let names: Vec<&str> = if at_root {
            root_names().collect()
        } else {
            NESTED.to_vec()
        };
        let at = Here {
            path: &path,
            parts: &parts,
            through: &through,
        };
        for name in names {
            let now = probe(volume, &folder, OsStr::new(name));
            if now == Now::Absent && !at_root {
                continue;
            }
            let kept = if at_root { Kept::Root } else { Kept::Nested };
            scan.items.push(at.item(OsStr::new(name), now, kept));
        }
        let listed = match volume.list(&folder) {
            Ok(listed) => listed,
            Err(error) => {
                unscanned(scan, &path, &error);
                continue;
            }
        };
        for child in listed {
            if child.kind == Kind::Folder {
                let mut key = parts.clone();
                key.push(child.name.clone());
                scan.folders.push(key);
            }
            match child.kind {
                Kind::Other => {}
                Kind::Link => match probe(volume, &folder, &child.name) {
                    Now::Absent => {}
                    now @ (Now::At(_) | Now::Shut | Now::Failed(_)) => {
                        scan.items.push(at.item(&child.name, now, Kept::Link));
                    }
                },
                Kind::Folder if !below => {}
                Kind::Folder => {
                    let next = path.join(&child.name);
                    let opened = volume
                        .open_folder(&folder, &child.name)
                        .and_then(|inner| Ok((volume.folder_id(&inner)?, inner)));
                    match opened {
                        Ok((id, inner)) => {
                            let mut deeper = parts.clone();
                            deeper.push(child.name);
                            let mut via = through.clone();
                            via.push(id);
                            stack.push((inner, next, deeper, via));
                        }
                        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                        Err(error) => unscanned(scan, &next, &error),
                    }
                }
            }
        }
    }
}

/// What is at `name` in `folder`, opened as itself.
fn probe<V: Volume>(volume: &V, folder: &V::Folder, name: &OsStr) -> Now {
    match volume.probe(folder, name) {
        Ok(None) => Now::Absent,
        Ok(Some(id)) => Now::At(id),
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Now::Shut,
        Err(error) => Now::Failed(io_failure(&error)),
    }
}

/// A folder the scan is in: its path, its parts below the root and the file IDs of the
/// folders it was reached through.
struct Here<'a> {
    path: &'a Path,
    parts: &'a [OsString],
    through: &'a [FileId],
}

impl Here<'_> {
    /// The item `name` in this folder.
    fn item(&self, name: &OsStr, now: Now, kept: Kept) -> Item {
        let mut key = self.parts.to_vec();
        key.push(name.to_owned());
        Item {
            key,
            path: self.path.join(name),
            within: self.parts.to_vec(),
            through: self.through.to_vec(),
            now,
            kept,
        }
    }
}

/// Notes the folder `path` as one the scan couldn't open or list.
fn unscanned(scan: &mut Scan, path: &Path, error: &io::Error) {
    let failure = io_failure(error);
    tracing::debug!(
        ?failure,
        "the set-aside's scan couldn't open or list a folder"
    );
    scan.unscanned.push(path.to_owned());
}

/// Whether two names are the same: on Windows as NTFS matches names (`CompareStringOrdinal`,
/// case ignored); elsewhere exactly, the stand-in's rule.
pub(super) fn same(a: &OsStr, b: &OsStr) -> bool {
    #[cfg(windows)]
    {
        super::ntfs::same_name(a, b)
    }
    #[cfg(not(windows))]
    {
        a == b
    }
}

/// Whether two keys are the same, part by part as [`same`] compares them.
pub(super) fn same_key(a: &[OsString], b: &[OsString]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(a, b)| same(a, b))
}

/// Whether the key `key` is `folder`'s key or below it, each part compared exactly (ordinal,
/// case kept). Not as [`same`] compares: a case-sensitive folder may hold `A` and `a`, and
/// `a\.git` isn't inside `A\.git` (round 4).
pub(super) fn inside(key: &[OsString], folder: &[OsString]) -> bool {
    key.starts_with(folder)
}
