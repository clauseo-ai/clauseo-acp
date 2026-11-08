//! The live watch's half of the set-aside: a git repository or an outbound link made while the
//! agent runs, moved aside at once through the same move, manifest and lock as the after-session
//! pass (`moves::set_aside`). The outer runner's watch (`runner/git_watch.rs`) hands over the
//! paths `ReadDirectoryChangesW` reported, or asks for a full rescan when the watch overflowed or
//! couldn't be armed.
//!
//! What is moved (names compared as `repo_names.rs` compares them):
//! - a `.git` (folder, file or link) at its shallowest `.git` part;
//! - a `HEAD` file made or renamed in, outside every `.git`: with `objects` and `refs` beside it,
//!   it makes the folder a bare repository git trusts when the lawyer owns the folder, and
//!   without it git finds none, so the lawyer's folder stays where it is;
//! - below a folder made or moved in (`ReadDirectoryChangesW` names only the folder, not the
//!   `.git` a folder moved in from the agent's temp folder holds), each `.git`, each bare
//!   repository's whole folder, and each `HEAD` the subtree holds;
//! - a link (junction or symbolic link) made or moved in that leads outside the matter, or
//!   whose target can't be read. A link within the matter is left to the after-session pass,
//!   which sets aside every new link.
//!
//! The watch ends the session before it moves anything ([`Hooks::end`], kill first): the
//! first item found ends the agent's job, so nothing alive races the moves that follow. Each
//! scan moves what it finds as it finds it (`scan::Found`), never after the whole tree, and a
//! scan that runs past [`SCAN_BUDGET`] ends the session too. An item gone before it could be
//! moved is listed ([`Live::gone`]), so the watch can end a session whose agent keeps renaming
//! one back and forth. A folder that becomes a link (`FILE_ACTION_MODIFIED` with the reparse
//! attribute: a junction set on an empty folder, `mklink /J`'s two steps) is checked as a link
//! made ([`Watched::relinked`]). What git reads from a folder a launch `commondir` names
//! (`scan::Kept::Common`) is moved when made, as a `.git` is.
//!
//! An item is known, and left, when the record (the launch pass's `record.json`) holds it at the
//! same key with the same file ID: the policy write-denies each `.git` there was at launch, and
//! the watch leaves what is inside one. So a lawyer's own `.git` whose folder the agent renamed
//! goes too, as the after-session pass would move it. With no record, or one an older version
//! wrote without its `HEAD`s, the watch's own scan when it starts stands in, and that is logged.
//! A rescan can't tell a new folder from the lawyer's, so of a bare repository it moves the `HEAD`.
//! Nothing here records anything: the after-session pass finds a moved item gone, as it finds
//! one the agent removed.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

use clauseo_protocol::MatterId;

use super::files::{FileId, Place, Record};
use super::moves::set_aside;
use super::names::Key;
use super::repo_names::{COMMON_NAMES, is_git, is_head, opens_as};
use super::run::{Refused, Volume};
use super::scan::{
    COMMON_KEY, Found, Item, Kept, Now, Step, plain, same_key, scan, scan_part_visiting,
    scan_visiting,
};
use super::{Entry, Reason, SetAsideFailure};
use crate::SetAsideWhat;

/// How long one scan may run while the agent could still be changing the tree: past it, the
/// session ends, and the scan carries on with nothing alive to race it.
pub(crate) const SCAN_BUDGET: Duration = Duration::from_secs(2);

/// One item the watch moved: its entry, its path below the watched folder, and what it was.
#[derive(Debug, Clone)]
pub(crate) struct Moved {
    pub(crate) entry: Entry,
    pub(crate) path: PathBuf,
    pub(crate) what: SetAsideWhat,
}

/// One item the watch found and couldn't move (a sharing violation while the agent holds a
/// handle, say): its full path, why, and what it was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Failed {
    pub(crate) path: PathBuf,
    pub(crate) failure: SetAsideFailure,
    pub(crate) what: SetAsideWhat,
}

/// What a live check did.
#[derive(Debug, Default)]
pub(crate) struct Live {
    pub(crate) moved: Vec<Moved>,
    pub(crate) failed: Vec<Failed>,
    /// Each item, by its path below the watched folder, that was reported or found and was gone
    /// before it could be moved.
    pub(crate) gone: Vec<PathBuf>,
}

/// Why the watch ends the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ending {
    /// It found an item to move: the job ends first, then the item moves.
    Found,
    /// A scan ran past [`SCAN_BUDGET`].
    Slow,
}

/// What a live check asks of the watch as it runs.
pub(crate) trait Hooks {
    /// Ends the session now: the agent's job ended and emptied before this returns, the first
    /// time; every later call does nothing more.
    fn end(&self, why: Ending);
    /// A monotonic reading, for [`SCAN_BUDGET`].
    fn now(&self) -> Duration;
}

/// One watched folder (the matter's, or its notes folder), the matter's set-aside, and what was
/// there at launch.
#[derive(Debug)]
pub(crate) struct Watched {
    root: PathBuf,
    place: Place,
    /// Each `.git`, `HEAD` and [`Kept::Common`] item there was at launch.
    known: Vec<(Key, FileId)>,
    /// Each link there was at launch.
    links: Vec<(Key, FileId)>,
    /// Each folder a launch `commondir` names, by its key (`[]` for the root).
    common: Vec<Key>,
}

impl Watched {
    /// The watch's view of `matter` at `root`: the record's, read under the matter's lock, or
    /// with none that holds the `HEAD`s, a scan's.
    pub(super) fn new(
        volume: &impl Volume,
        root: &Path,
        data_dir: &Path,
        matter: &MatterId,
    ) -> Watched {
        let place = Place::new(data_dir, matter);
        let record = place.lock().and_then(|held| {
            let record = place.read_record();
            drop(held);
            record
        });
        match record {
            Ok(Some(record)) if record.holds_heads() => Watched::of_record(root, place, record),
            Ok(_) | Err(_) => {
                tracing::warn!("no set-aside record of the launch's git parts; scanning instead");
                Watched::scanned(volume, root, place)
            }
        }
    }

    /// The watch's view of a folder the record doesn't cover (the matter's notes folder): what a
    /// scan finds there now, moved into `matter`'s set-aside.
    pub(super) fn new_scanned(
        volume: &impl Volume,
        root: &Path,
        data_dir: &Path,
        matter: &MatterId,
    ) -> Watched {
        Watched::scanned(volume, root, Place::new(data_dir, matter))
    }

    fn of_record(root: &Path, place: Place, record: Record) -> Watched {
        let common: Vec<Key> = record
            .names
            .keys()
            .filter(|key| key.first().is_some_and(|first| first == COMMON_KEY))
            .map(|key| key.get(1..).unwrap_or_default().to_vec())
            .collect();
        let mut watched = Watched {
            root: root.to_owned(),
            place,
            known: Vec::new(),
            links: record.links.into_iter().collect(),
            common,
        };
        watched.known = record
            .names
            .into_iter()
            .filter_map(|(key, id)| Some((key, id?)))
            .filter(|(key, _)| repo_part(key) || watched.common_at(key).is_some())
            .collect();
        watched
    }

    fn scanned(volume: &impl Volume, root: &Path, place: Place) -> Watched {
        let found = scan(volume, root);
        let at = |item: &Item| match item.now {
            Now::At(id) => Some((item.key.clone(), id)),
            Now::Absent | Now::Shut | Now::Failed(_) => None,
        };
        let (links, names): (Vec<&Item>, Vec<&Item>) =
            found.items.iter().partition(|item| item.kept == Kept::Link);
        let known = names
            .into_iter()
            .filter(|i| repo_part(&i.key) || i.kept == Kept::Common)
            .filter_map(at);
        Watched {
            root: root.to_owned(),
            place,
            known: known.collect(),
            links: links.into_iter().filter_map(at).collect(),
            common: found.common,
        }
    }

    /// The folder watched.
    pub(super) fn root(&self) -> &Path {
        &self.root
    }

    /// Checks each path the watch reported made or renamed, relative to the root.
    pub(super) fn changed(
        &self,
        volume: &impl Volume,
        paths: &[PathBuf],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        let mut live = Live::default();
        let mut run = Run::new(self, volume, now, hooks, &mut live);
        for path in paths {
            let Some(key) = key_of(path) else {
                tracing::debug!("the watch reported a path that isn't plain; passed over");
                continue;
            };
            let full = self.path(&key);
            let repo = key.iter().position(|part| is_git(part)).map(|at| at + 1);
            if let Some(end) = repo.or_else(|| self.common_at(&key)) {
                let part = key.get(..end).unwrap_or_default();
                run.check(part, SetAsideWhat::GitRepository);
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&full) else {
                continue;
            };
            let link = meta.file_type().is_symlink();
            let head = key.last().is_some_and(|name| is_head(name)) && !meta.is_dir();
            if link && self.leads_out(&full) {
                run.check(&key, SetAsideWhat::LinkOut);
            } else if meta.is_dir() && !link {
                run.below(&key);
            } else if head {
                let folder = full.parent().map(|folder| volume.file_id(folder));
                if !matches!(folder, Some(Ok(Some(id))) if self.git_folder(id)) {
                    run.check(&key, SetAsideWhat::GitRepository);
                }
            }
        }
        live
    }

    /// Checks each path the watch reported changed in its attributes as a reparse point now:
    /// a folder that became a junction or symbolic link leading out, or whose target can't be
    /// read, is a link made. Anything else (a cloud file's placeholder, say) is passed over.
    pub(super) fn relinked(
        &self,
        volume: &impl Volume,
        paths: &[PathBuf],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        let mut live = Live::default();
        let mut run = Run::new(self, volume, now, hooks, &mut live);
        for key in paths.iter().filter_map(|path| key_of(path)) {
            let full = self.path(&key);
            let link = std::fs::symlink_metadata(&full).is_ok_and(|m| m.file_type().is_symlink());
            if link && self.leads_out(&full) {
                run.check(&key, SetAsideWhat::LinkOut);
            }
        }
        live
    }

    /// Scans the whole folder and moves, as it finds it, what the watch moves that it doesn't
    /// know: after an overflow, a watch armed again, at the session's end, or every few seconds
    /// while the watch can't be armed.
    pub(super) fn rescan(&self, volume: &impl Volume, now: SystemTime, hooks: &impl Hooks) -> Live {
        let mut live = Live::default();
        let mut run = Run::new(self, volume, now, hooks, &mut live);
        let found = scan_visiting(volume, &self.root, &mut |found| run.visit(found, false));
        if !found.unscanned.is_empty() {
            tracing::warn!("the watch's rescan couldn't open every folder");
        }
        live
    }

    /// Tries again what a check couldn't move, once the session ended.
    pub(super) fn retry(
        &self,
        volume: &impl Volume,
        failed: &[Failed],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        let mut live = Live::default();
        let mut run = Run::new(self, volume, now, hooks, &mut live);
        for each in failed {
            match each.path.strip_prefix(&self.root).ok().and_then(key_of) {
                Some(key) => run.check(&key, each.what),
                None => run.live.failed.push(Failed {
                    failure: SetAsideFailure::Changed,
                    ..each.clone()
                }),
            }
        }
        live
    }

    /// The length of the part of `key` that is git's in a folder a launch `commondir` names:
    /// the folder itself, made while it was missing, or one of [`COMMON_NAMES`] in it.
    fn common_at(&self, key: &[OsString]) -> Option<usize> {
        self.common.iter().find_map(|target| {
            let starts = key.len() >= target.len()
                && same_key(key.get(..target.len()).unwrap_or_default(), target);
            let named = key
                .get(target.len())
                .is_some_and(|name| COMMON_NAMES.iter().any(|common| opens_as(name, common)));
            match (starts, key.len() == target.len()) {
                (true, true) if !target.is_empty() => Some(target.len()),
                (true, false) if named => Some(target.len() + 1),
                _ => None,
            }
        })
    }

    /// Whether the record holds the item at `key` with the file ID `id`, as a link for
    /// [`SetAsideWhat::LinkOut`], else as a git part.
    fn knows_as(&self, key: &[OsString], id: FileId, what: SetAsideWhat) -> bool {
        match what {
            SetAsideWhat::LinkOut => holds(&self.links, key, Some(id)),
            SetAsideWhat::GitRepository => holds(&self.known, key, Some(id)),
        }
    }

    /// Whether `id` is a launch `.git`'s: one the lawyer renamed is still denied, and a `HEAD`
    /// in it is that `.git`'s own.
    fn git_folder(&self, id: FileId) -> bool {
        self.known
            .iter()
            .any(|(key, at)| *at == id && ends_in_git(key))
    }

    /// Whether the link at `link` leads outside the watched folder, or its target can't be
    /// read. Its target is resolved by its words alone, never opened: a link out is moved
    /// before anything follows it.
    fn leads_out(&self, link: &Path) -> bool {
        let Ok(target) = std::fs::read_link(link) else {
            return true;
        };
        let full = link
            .parent()
            .map_or_else(|| target.clone(), |at| at.join(&target));
        let (full, root) = (plain(&full), plain(&self.root));
        !(full.len() >= root.len()
            && full
                .iter()
                .zip(&root)
                .all(|(a, b)| a.eq_ignore_ascii_case(b)))
    }

    /// The full path of `key`.
    fn path(&self, key: &[OsString]) -> PathBuf {
        key.iter()
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// An item at `key` for [`Run::move_one`].
    fn item(&self, key: &[OsString], now: Now) -> Item {
        Item {
            key: key.to_vec(),
            path: self.path(key),
            within: key
                .get(..key.len().saturating_sub(1))
                .unwrap_or_default()
                .to_vec(),
            through: Vec::new(),
            now,
            kept: Kept::Nested,
        }
    }
}

/// One live check in progress: what it moved so far, and when it began.
struct Run<'a, V: Volume, H: Hooks> {
    watched: &'a Watched,
    volume: &'a V,
    now: SystemTime,
    hooks: &'a H,
    live: &'a mut Live,
    began: Duration,
    slow: bool,
}

impl<'a, V: Volume, H: Hooks> Run<'a, V, H> {
    fn new(
        watched: &'a Watched,
        volume: &'a V,
        now: SystemTime,
        hooks: &'a H,
        live: &'a mut Live,
    ) -> Self {
        let began = hooks.now();
        Run {
            watched,
            volume,
            now,
            hooks,
            live,
            began,
            slow: false,
        }
    }

    /// Scans below the folder `key`, made or moved in, moving as it goes. A folder gone is
    /// nothing to scan.
    fn below(&mut self, key: &[OsString]) {
        let (volume, root) = (self.volume, self.watched.root.clone());
        match scan_part_visiting(volume, &root, key, true, &mut |found| {
            self.visit(found, true)
        }) {
            Ok(_) => {}
            Err(error) => {
                let error = error.kind();
                tracing::debug!(%error, "a reported folder the watch can't scan");
            }
        }
    }

    /// What the scan found, moved at once when the watch moves it and doesn't know it: in a
    /// folder made or moved in (`new`), a git folder whole; what is below a `.git` is left to
    /// that one (the known one's tree is denied, an unknown one goes whole). [`Step::Prune`]
    /// once moved, so nothing below it is scanned.
    fn visit(&mut self, found: Found<'_>, new: bool) -> Step {
        if !self.slow && self.hooks.now().saturating_sub(self.began) > SCAN_BUDGET {
            self.slow = true;
            self.hooks.end(Ending::Slow);
        }
        let watched = self.watched;
        let in_git = |id: Option<FileId>| id.is_some_and(|id| watched.git_folder(id));
        match found {
            Found::Folder => Step::Go,
            Found::Bare(folder, id) => {
                if !new || in_git(id) || below_git(folder) || self.head_known(folder) {
                    return Step::Go;
                }
                let item = watched.item(folder, Now::Shut);
                self.step(&item, SetAsideWhat::GitRepository)
            }
            Found::Item(item) => {
                if below_git(&item.key) {
                    return Step::Go;
                }
                let what = match item.kept {
                    Kept::Link if watched.leads_out(&item.path) => SetAsideWhat::LinkOut,
                    Kept::Head if in_git(item.through.last().copied()) => return Step::Go,
                    Kept::Head | Kept::Common => SetAsideWhat::GitRepository,
                    Kept::Root | Kept::Nested if ends_in_git(&item.key) => {
                        SetAsideWhat::GitRepository
                    }
                    Kept::Root | Kept::Nested | Kept::Link => return Step::Go,
                };
                match item.now {
                    Now::Absent => Step::Go,
                    Now::At(id) if watched.knows_as(&item.key, id, what) => Step::Go,
                    Now::At(_) | Now::Shut | Now::Failed(_) => self.step(item, what),
                }
            }
        }
    }

    /// Moves `item`; [`Step::Prune`] when it moved.
    fn step(&mut self, item: &Item, what: SetAsideWhat) -> Step {
        if self.move_one(item, what) {
            Step::Prune
        } else {
            Step::Go
        }
    }

    /// Whether the `HEAD` in the folder `folder` is one the watch knows.
    fn head_known(&self, folder: &[OsString]) -> bool {
        let mut key = folder.to_vec();
        key.push(OsString::from("HEAD"));
        matches!(
            self.volume.file_id(&self.watched.path(&key)),
            Ok(Some(id)) if self.watched.knows_as(&key, id, SetAsideWhat::GitRepository)
        )
    }

    /// Moves the item at `key` as `what` unless nothing is there or the watch knows it there.
    /// One the watch was told of and that is gone is listed in [`Live::gone`].
    fn check(&mut self, key: &[OsString], what: SetAsideWhat) {
        let now_at = match self.volume.file_id(&self.watched.path(key)) {
            Ok(None) => {
                self.live.gone.push(key.iter().collect());
                return;
            }
            Ok(Some(id)) if self.watched.knows_as(key, id, what) => return,
            Ok(Some(id)) => Now::At(id),
            Err(_) => Now::Shut,
        };
        let item = self.watched.item(key, now_at);
        self.move_one(&item, what);
    }

    /// Ends the session, then moves `item` as `what` under the matter's lock into this pass's
    /// folder at its own path, as the after-session pass does. Whether it moved. One gone
    /// meanwhile is listed in [`Live::gone`].
    fn move_one(&mut self, item: &Item, what: SetAsideWhat) -> bool {
        // Kill first: nothing of the agent's is alive to race the move, or to put another back.
        self.hooks.end(Ending::Found);
        let watched = self.watched;
        let seen = match what {
            SetAsideWhat::LinkOut => holds(&watched.links, &item.key, None),
            SetAsideWhat::GitRepository => holds(&watched.known, &item.key, None),
        };
        let reason = if seen { Reason::Changed } else { Reason::New };
        let place = &watched.place;
        let moved = place
            .lock()
            .map_err(|failure| Refused::at(failure, &item.path))
            .and_then(|held| {
                let moved = set_aside(
                    self.volume,
                    place,
                    &item.path,
                    &item.within,
                    reason,
                    self.now,
                );
                drop(held);
                moved
            });
        match moved {
            Ok(entry) => {
                self.live.moved.push(Moved {
                    entry,
                    path: item.key.iter().collect(),
                    what,
                });
                true
            }
            Err(refused) if refused.failure == SetAsideFailure::Gone => {
                tracing::debug!("an item went before the watch could set it aside");
                self.live.gone.push(item.key.iter().collect());
                false
            }
            Err(refused) => {
                self.live.failed.push(Failed {
                    path: item.path.clone(),
                    failure: refused.failure,
                    what,
                });
                false
            }
        }
    }
}

/// Whether `key` is below a `.git`.
fn below_git(key: &[OsString]) -> bool {
    let parents = key.get(..key.len().saturating_sub(1)).unwrap_or_default();
    parents.iter().any(|part| is_git(part))
}

/// Whether `list` holds `key` (as [`same_key`] compares), with the file ID `id` when one is
/// given.
fn holds(list: &[(Key, FileId)], key: &[OsString], id: Option<FileId>) -> bool {
    list.iter()
        .any(|(known, at)| id.is_none_or(|id| *at == id) && same_key(known, key))
}

/// Whether `key`'s own name opens as `.git`.
fn ends_in_git(key: &[OsString]) -> bool {
    key.last().is_some_and(|name| is_git(name))
}

/// Whether `key` names a `.git`, or a `HEAD` outside every `.git`: what the watch knows.
fn repo_part(key: &[OsString]) -> bool {
    ends_in_git(key) || key.last().is_some_and(|name| is_head(name))
}

/// `path`'s parts, when each is a plain name (no root, drive, `.` or `..`).
fn key_of(path: &Path) -> Option<Key> {
    let mut key = Vec::new();
    for part in path.components() {
        match part {
            Component::Normal(name) => key.push(name.to_owned()),
            Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir => {
                return None;
            }
        }
    }
    (!key.is_empty()).then_some(key)
}
