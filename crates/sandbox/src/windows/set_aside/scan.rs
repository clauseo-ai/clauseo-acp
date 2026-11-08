//! The set-aside's own scan of the matter tree (S15 `PR3b` round 3, R1 and R2). It reads and
//! never writes, has no cap, and never follows a link: each folder is opened relative to its
//! parent's handle, and a link is recorded, never entered. It doesn't depend on the ACL walk,
//! so a walk that refuses (past its set-aside cap, say) never stops the check.
//!
//! In each folder it visits, it probes each protected name by opening it relative to that
//! folder's handle, as git and Claude would open it: at the root each of
//! [`PROTECTED_NAMES`](crate::PROTECTED_NAMES) and `.git`, at every depth below each of
//! [`NESTED_NAMES`]. On NTFS the open ignores case as NTFS does, so whatever the volume
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
use std::path::{Component, Path, PathBuf};

use super::SetAsideFailure;
use super::files::{FileId, io_failure};
use super::names::Key;
use super::repo_names::{bare_layout, git_lookalike, is_commondir, is_git, is_head};
use super::run::Volume;
use crate::PROTECTED_NAMES;

mod common;
mod locate;
mod reach;

use common::{common_items, common_target};
#[cfg(test)]
pub(super) use locate::remote;
pub(super) use locate::{Last, locate};
pub(super) use reach::{Missed, open_key, reach};

/// Also probed at the root: hooks in a `.git` the agent made would run in the lawyer's own git.
const ALSO: [&str; 1] = [".git"];

/// Probed in every folder below the root: the policy's
/// [`PROTECTED_AT_ANY_DEPTH`](crate::PROTECTED_AT_ANY_DEPTH), the names it write-denies at any
/// depth where they exist at launch (S15 `PR4a2`), so the set-aside catches the ones made or
/// replaced since. The one list below the root: the full scan and the fast pass's re-probe of
/// each folder `clean_close` names ([`scan_part`]) both probe these. Each is one open per
/// folder, matched as the volume matches names (on NTFS, case ignored). `CLAUDE.local.md` and
/// `AGENTS.override.md` aren't probed below the root: the two opens cost 6.0% more on average
/// (4.8% to 6.8%, three runs) on 50,000 files ten to a folder (`tests/timing.rs`), past the 5%
/// D-B17 allows.
pub(super) const NESTED_NAMES: [&str; 5] = crate::PROTECTED_AT_ANY_DEPTH;

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
    /// A `HEAD` (a file or a link) outside every `.git`, from its folder's listing: with
    /// `objects` and `refs`, or `commondir`, beside it, a git folder git would trust in a
    /// lawyer's folder. Checked only
    /// against a record that holds them ([`HEADS_RECORDED`](super::files::HEADS_RECORDED)).
    Head,
    /// A link, in [`Record::links`](super::files::Record::links).
    Link,
    /// What git reads from a folder a `commondir` names
    /// ([`COMMON_NAMES`](super::repo_names::COMMON_NAMES)), or that folder
    /// itself while it is missing: made there, git would read it as the lawyer's (9a). Checked
    /// only against a record that holds the `HEAD`s, as [`Kept::Head`] is.
    Common,
}

/// The record's key for the folder a `commondir` names, `[COMMON_KEY, parts...]`, kept with no
/// file ID so the live watch knows the folder: no name on NTFS holds a `:`.
pub(super) const COMMON_KEY: &str = ":common";

/// What a scan tells its visitor as it finds it, before it goes on.
#[derive(Clone, Copy)]
pub(super) enum Found<'a> {
    /// A folder about to be scanned: the visitor can keep time.
    Folder,
    /// A folder whose listing is a git folder's ([`Scan::bare`]), before anything in it.
    Bare(&'a [OsString], Option<FileId>),
    /// An item, as pushed onto [`Scan::items`].
    Item(&'a Item),
}

/// What the visitor answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// Go on as before.
    Go,
    /// The visitor moved it: nothing below it is scanned.
    Prune,
    /// The scan stops here, scanning nothing more ([`Scan::stopped`]).
    Stop,
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
    /// Each folder below the root whose listing is a git folder's (`HEAD` with `objects` and
    /// `refs`, or with `commondir`: `repo_names::bare_layout`), and its file ID.
    pub(super) bare: Vec<(Key, Option<FileId>)>,
    /// The key of each folder in the matter a git folder's `commondir` names, outside every
    /// `.git` (`[]` for the matter's own folder), whether or not it is there.
    pub(super) common: Vec<Key>,
    /// Whether the visitor stopped the scan ([`Step::Stop`]) before it was done.
    pub(super) stopped: bool,
}

/// Scans the matter tree below `root` through `volume`; see the module's docs.
pub(super) fn scan<V: Volume>(volume: &V, root: &Path) -> Scan {
    scan_probing(volume, root, &NESTED_NAMES)
}

/// [`scan`], telling `visit` of each item and git folder as it finds it.
pub(super) fn scan_visiting<V: Volume>(
    volume: &V,
    root: &Path,
    visit: &mut dyn FnMut(Found<'_>) -> Step,
) -> Scan {
    scan_with(volume, root, &NESTED_NAMES, visit)
}

/// [`scan`], probing `nested` below the root: the timing test compares lists.
pub(super) fn scan_probing<V: Volume>(volume: &V, root: &Path, nested: &[&str]) -> Scan {
    scan_with(volume, root, nested, &mut |_| Step::Go)
}

/// [`scan`], probing `nested` and telling `visit`.
fn scan_with<V: Volume>(
    volume: &V,
    root: &Path,
    nested: &[&str],
    visit: &mut dyn FnMut(Found<'_>) -> Step,
) -> Scan {
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
        (true, nested),
        visit,
    );
    if !scan.stopped {
        common_items(volume, root, &mut scan, visit);
    }
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
    scan_part_visiting(volume, root, parts, below, &mut |_| Step::Go)
}

/// [`scan_part`], telling `visit` of each item and git folder as it finds it.
pub(super) fn scan_part_visiting<V: Volume>(
    volume: &V,
    root: &Path,
    parts: &[OsString],
    below: bool,
    visit: &mut dyn FnMut(Found<'_>) -> Step,
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
        (below, &NESTED_NAMES),
        visit,
    );
    if !scan.stopped {
        common_items(volume, root, &mut scan, visit);
    }
    Ok(Some(scan))
}

/// A folder the scan will visit: it, its path, its parts below the root and the IDs of the
/// folders it was reached through.
type Visit<F> = (F, PathBuf, Vec<OsString>, Vec<FileId>);

/// Scans `start`, and with `below` every folder below it, into `scan`, probing `nested` in
/// each folder below the root and telling `visit` of each git folder and item as it finds it:
/// a git folder before anything in it, so the visitor can move it whole and skip its insides.
/// [`Step::Stop`] ends it before the next item or folder, every folder it opened closed.
fn scan_from<V: Volume>(
    volume: &V,
    scan: &mut Scan,
    start: Visit<V::Folder>,
    (below, nested): (bool, &[&str]),
    visit: &mut dyn FnMut(Found<'_>) -> Step,
) {
    let stopped = std::cell::Cell::new(false);
    let visit = &mut |found: Found<'_>| {
        let step = visit(found);
        stopped.set(stopped.get() || step == Step::Stop);
        step
    };
    let mut stack = vec![start];
    while let Some((folder, path, parts, through)) = stack.pop() {
        if stopped.get() || visit(Found::Folder) != Step::Go {
            continue;
        }
        let at_root = parts.is_empty();
        let in_git = parts.iter().any(|part| is_git(part));
        let listed = volume.list(&folder);
        if let Ok(listed) = &listed
            && !at_root
            && bare_layout(listed.iter().map(|c| (c.name.as_os_str(), c.kind)))
        {
            let id = through.last().copied();
            scan.bare.push((parts.clone(), id));
            // The visitor first, before anything in the folder is read: the live watch ends the
            // session at an unknown git folder, so an agent's oplock on its `commondir` can't
            // hold the watch up while the agent runs.
            if visit(Found::Bare(&parts, id)) != Step::Go {
                continue;
            }
            let root = scan_root(&path, &parts);
            if let Some(common) = listed
                .iter()
                .find(|c| c.kind == Kind::Other && is_commondir(&c.name))
                .and_then(|c| common_target(volume, &folder, (root, &path), &c.name))
                && !scan.common.contains(&common)
            {
                scan.common.push(common);
            }
        }
        let names: Vec<&str> = if at_root {
            root_names().collect()
        } else {
            nested.to_vec()
        };
        let at = Here {
            path: &path,
            parts: &parts,
            through: &through,
        };
        let mut pruned: Vec<OsString> = Vec::new();
        for name in names {
            let now = probe(volume, &folder, OsStr::new(name));
            if now == Now::Absent && !at_root {
                continue;
            }
            let kept = if at_root { Kept::Root } else { Kept::Nested };
            if found(scan, visit, at.item(OsStr::new(name), now, kept)) {
                pruned.push(OsString::from(name));
            }
        }
        let listed = match listed {
            Ok(listed) => listed,
            Err(error) => {
                unscanned(scan, &path, &error);
                continue;
            }
        };
        for child in listed {
            if stopped.get() {
                break;
            }
            // From the listing: a `HEAD` file, and a name only stripping makes `.git`, which
            // the probes above don't open.
            let named = match child.kind {
                Kind::Other | Kind::Link if !in_git && is_head(&child.name) => Some(Kept::Head),
                Kind::Other | Kind::Folder if git_lookalike(&child.name) => Some(Kept::Nested),
                Kind::Other | Kind::Folder | Kind::Link => None,
            };
            if let Some(kept) = named {
                let now = probe(volume, &folder, &child.name);
                if now != Now::Absent && found(scan, visit, at.item(&child.name, now, kept)) {
                    pruned.push(child.name.clone());
                }
            }
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
                        found(scan, visit, at.item(&child.name, now, Kept::Link));
                    }
                },
                Kind::Folder if !below || pruned.iter().any(|p| same(p, &child.name)) => {}
                // One folder may hold very many: the visitor keeps time before each is opened.
                Kind::Folder if visit(Found::Folder) != Step::Go => {}
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
    scan.stopped = stopped.get();
}

/// Pushes `item` onto the scan and tells `visit`; whether the visitor moved it.
fn found(scan: &mut Scan, visit: &mut dyn FnMut(Found<'_>) -> Step, item: Item) -> bool {
    scan.items.push(item);
    scan.items
        .last()
        .is_some_and(|item| visit(Found::Item(item)) == Step::Prune)
}

/// The matter's folder, from the path of a folder `parts` below it.
fn scan_root<'a>(path: &'a Path, parts: &[OsString]) -> &'a Path {
    parts.iter().fold(path, |at, _| at.parent().unwrap_or(at))
}

/// `path` as text parts, its `..` and `.` worked out by their words, with a verbatim prefix
/// (`\\?\C:\`, `\\?\UNC\`, `\??\`) read as the plain one, for comparing a link's or a
/// `commondir`'s target with the matter's folder.
pub(super) fn plain(path: &Path) -> Vec<String> {
    let text = path.to_string_lossy();
    let text = if let Some(rest) = text
        .strip_prefix(r"\\?\UNC\")
        .or_else(|| text.strip_prefix(r"\??\UNC\"))
    {
        format!(r"\\{rest}")
    } else {
        let rest = text
            .strip_prefix(r"\\?\")
            .or_else(|| text.strip_prefix(r"\??\"));
        rest.unwrap_or(&text).to_owned()
    };
    let mut parts: Vec<String> = Vec::new();
    for part in Path::new(&text).components() {
        match part {
            Component::ParentDir => {
                if parts.len() > 1 {
                    parts.pop();
                }
            }
            Component::CurDir => {}
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                parts.push(part.as_os_str().to_string_lossy().into_owned());
            }
        }
    }
    parts
}

/// What is at `name` in `folder`, opened as itself.
fn probe<V: Volume>(volume: &V, folder: &V::Folder, name: &OsStr) -> Now {
    match volume.probe(folder, name) {
        Ok(None) => Now::Absent,
        Ok(Some(id)) => Now::At(id),
        Err(error) => unread(&error),
    }
}

/// What an item whose read failed with `error` is: [`Now::Shut`] when access was denied.
fn unread(error: &io::Error) -> Now {
    if error.kind() == io::ErrorKind::PermissionDenied {
        Now::Shut
    } else {
        Now::Failed(io_failure(error))
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
