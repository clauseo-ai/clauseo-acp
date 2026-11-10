//! The watch's decisions: which events and which entries of a scan are found, and, once the
//! session has ended, set aside.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::aside::{Aside, Stuck};
use super::state::{Baseline, Id, Known};
use super::{
    ARRIVED, Asks, Event, Fs, Kind, Launch, Meta, Phase, RESCAN, head_of, is_git, repo_head,
};
use crate::GitItem;
use crate::policy::{name_key, path_key};

mod names;
mod outcome;
mod sweep;
mod volumes;
mod walk;

use outcome::Found;
pub(crate) use sweep::Sweep;
use volumes::{Met, Seen};
#[cfg(test)]
pub(super) use walk::walk;
use walk::{Next, Visit, walk_with};

/// The most entries the scans of one batch's arrived folders list while the agent may run:
/// past it the scan stops and the session ends, so a huge folder can't stall the watch while
/// the agent works. The final scan, once the session has ended, has no cap.
pub(crate) const SCAN_BUDGET: usize = 20_000;

/// Items an event reported that were gone when it was checked, in every watch of this host.
static GONE: AtomicU64 = AtomicU64::new(0);

/// How many items events reported that were gone when checked, in every watch since the host
/// started: a rename in and out again before the check leaves only this trace.
pub(crate) fn gone_total() -> u64 {
    GONE.load(Ordering::Relaxed)
}

/// One session's watch of one matter.
pub(crate) struct Watch<F> {
    fs: F,
    matter: PathBuf,
    aside: Aside,
    base: Baseline,
    /// The launch's repositories, left alone while each keeps its file ID: its `.git`s and its
    /// repository-shaped folders, by path key.
    repos: HashMap<PathBuf, Id>,
    /// The launch's folders holding a `HEAD` but not repository-shaped, by path key: left alone
    /// while they keep their ID and stay that way.
    heads: HashMap<PathBuf, Id>,
    /// The launch's links leading out of the matter or nowhere, by path key.
    links: HashMap<PathBuf, Id>,
    /// The paths at the launch repositories' `commondir` targets, by path key, with the ID of
    /// what was there at launch.
    common: HashMap<PathBuf, (PathBuf, Option<Id>)>,
    /// Folders never entered: the volume set-aside folder, when it is in the matter.
    skip: HashSet<PathBuf>,
    /// The file IDs of the matter's folders at the start.
    folders: HashSet<Id>,
    /// Every folder counts as one that existed at the start: the scan of a session whose watch
    /// stopped early, whose folders were never kept.
    all_old: bool,
    /// The 30 s pass's folders and where it stopped.
    sweep: Sweep,
    /// Items this watch's events reported that were gone when checked.
    gone: u64,
    /// Called once per folder any walk lists: the runner's heartbeat.
    progress: Progress,
    /// The volumes the launch met under the folder (`check/volumes.rs`); `None` for a record
    /// made before they were kept.
    volumes: Option<Seen>,
}

/// A hook called once per folder listed.
pub(crate) type Progress = Arc<dyn Fn() + Send + Sync>;

/// A folder's listing.
type Entries = [(OsString, Kind)];

/// One item to set aside.
type Item = (PathBuf, GitItem, Kind);

impl<F: Fs> Watch<F> {
    /// The watch of `matter`, setting aside through `aside`. The launch's items' file IDs are
    /// read now, and the matter is walked once for its folders, repositories and `HEAD`
    /// folders (kept as the launch's only when [`Launch::keep_repos`]), so call it before the
    /// agent starts.
    pub(crate) fn new(fs: F, matter: PathBuf, aside: Aside, launch: &Launch) -> Watch<F> {
        Watch::new_with_progress(fs, matter, aside, launch, Arc::new(|| {}))
    }

    /// [`Watch::new`], calling `progress` once per folder listed, from the launch walk on
    /// ([`Watch::on_progress`]).
    pub(crate) fn new_with_progress(
        fs: F,
        matter: PathBuf,
        aside: Aside,
        launch: &Launch,
        progress: Progress,
    ) -> Watch<F> {
        let known = |paths: &[PathBuf]| -> Vec<Known> {
            paths
                .iter()
                .filter_map(|path| Some(Known::new(path, fs.meta(path)?.id)))
                .collect()
        };
        let mut base = Baseline {
            repos: known(&launch.git),
            links: known(&launch.links),
            common: launch
                .common
                .iter()
                .map(|path| (path.into(), fs.meta(path).map(|meta| meta.id)))
                .collect(),
            skip: launch
                .skip
                .iter()
                .map(|path| path.as_path().into())
                .collect(),
            ..Baseline::default()
        };
        let mut folders = HashSet::new();
        let mut sweep = Sweep::default();
        let mut repo_dirs: HashSet<PathBuf> = HashSet::new();
        let skip: HashSet<PathBuf> = launch.skip.iter().map(|p| path_key(p)).collect();
        let met = RefCell::new(Met::default());
        let mut folder = |dir: &Path, meta: Meta, entries: &Entries| {
            folders.insert(meta.id);
            met.borrow_mut().device(meta.id.0);
            if !launch.keep_repos {
                // Nothing the profile doesn't make read-only is the launch's: notes.
            } else if repo_head(entries).is_some() {
                base.repos.push(Known::new(dir, meta.id));
                repo_dirs.insert(dir.to_owned());
            } else if head_of(entries).is_some() {
                base.heads.push(Known::new(dir, meta.id));
            }
            // The 30 s pass, as a scan, never enters a launch repository.
            if !dir.ancestors().any(|above| repo_dirs.contains(above)) {
                sweep.add(dir, Some(meta.id), fs.dated(dir).map(|(_, stamp)| stamp));
            }
            Next::Below
        };
        let mut visit = Visit {
            skip: &|dir, _| skip.contains(&path_key(dir)),
            folder: &mut folder,
            unreadable: &mut |_, _| Next::Past,
            refused: &mut |dir| {
                met.borrow_mut().refused(dir);
                Next::Past
            },
            progress: &*progress,
        };
        walk_with(&fs, &matter, &mut visit);
        base.volumes = Some(met.into_inner().record());
        let mut watch = Watch::resume(fs, matter, aside, base);
        watch.folders = folders;
        watch.all_old = false;
        watch.sweep = sweep;
        watch.on_progress(progress);
        watch
    }

    /// Calls `f` once per folder any later walk lists: event scans, full scans, the 30 s pass
    /// and the final scan. A no-op until set.
    pub(crate) fn on_progress(&mut self, f: Progress) {
        self.progress = f;
    }

    /// The watch of a session from its recorded `base`, for its final scan: every folder
    /// counts as old. It sets aside repository shapes only, never a whole folder of the
    /// lawyer's: each `.git` not the launch's, the `HEAD` of a folder that was not a launch
    /// repository (a repository-shaped one keeps the rest), each link out of the matter and
    /// each new `commondir` path. A repository the lawyer made since the session died loses
    /// its `.git` or its `HEAD` the same way.
    pub(crate) fn resume(fs: F, matter: PathBuf, aside: Aside, base: Baseline) -> Watch<F> {
        let keyed = |list: &[Known]| -> HashMap<PathBuf, Id> {
            list.iter()
                .map(|known| (path_key(&known.path()), known.id))
                .collect()
        };
        Watch {
            repos: keyed(&base.repos),
            heads: keyed(&base.heads),
            links: keyed(&base.links),
            common: base
                .common
                .iter()
                .map(|(path, id)| (path_key(&path.path()), (path.path(), *id)))
                .collect(),
            skip: base
                .skip
                .iter()
                .map(|path| path_key(&path.path()))
                .collect(),
            folders: HashSet::new(),
            all_old: true,
            sweep: Sweep::default(),
            gone: 0,
            progress: Arc::new(|| {}),
            volumes: base.volumes.as_ref().map(Seen::of),
            fs,
            matter,
            aside,
            base,
        }
    }

    /// The matter.
    pub(crate) fn matter(&self) -> &Path {
        &self.matter
    }

    /// The file system.
    pub(crate) fn fs(&self) -> &F {
        &self.fs
    }

    /// What the launch knew, for the matter's state file.
    pub(crate) fn baseline(&self) -> &Baseline {
        &self.base
    }

    /// The runner ended the session for a find.
    pub(crate) fn ended_session(&mut self) {
        self.aside.ended_by_us = true;
    }

    /// The runner lost the stream and ends the session: the lawyer is told.
    pub(crate) fn lost_stream(&mut self) {
        self.aside.notices.denied(crate::Denial::GitWatchLost);
    }

    /// The items that could neither move nor be neutralised in place, taken.
    pub(crate) fn take_stuck(&mut self) -> Vec<Stuck> {
        std::mem::take(&mut self.aside.stuck)
    }

    /// How many items this watch's events reported that were gone when checked.
    pub(crate) fn gone(&self) -> u64 {
        self.gone
    }

    /// Checks one batch of events, while the agent may run: finds end the session, whether
    /// or not the item is still there. Each event's path is checked by name first
    /// (`check/names.rs`), reading no folder: one hit ends the session at once, with no
    /// scan. Else each event is checked, its arrived folders scanned within [`SCAN_BUDGET`]
    /// entries in all; past it the session ends too.
    pub(crate) fn events(&mut self, events: &[Event]) -> Asks {
        let rescan = events.iter().any(|event| event.flags & RESCAN != 0);
        if let Some(hit) = events.iter().find(|event| names::named(self, event)) {
            for event in events {
                self.count_gone(event);
            }
            tracing::warn!(
                path = %hit.path.display(),
                "an event named a git item in the matter; ending the session"
            );
            return Asks { rescan, end: true };
        }
        let mut found = Found::default();
        let budget = Cell::new(SCAN_BUDGET);
        let mut rescan = rescan;
        for event in events {
            self.count_gone(event);
            let asks = self.event(event, &mut found, &budget);
            rescan |= asks.rescan;
            if asks.end {
                tracing::warn!(
                    path = %event.path.display(), budget = SCAN_BUDGET,
                    "the git watch's scan of an arrived folder ran past its budget; ending the \
                     session"
                );
                return Asks { rescan, end: true };
            }
            // The first find ends the session: the rest of the batch can wait for the end.
            if found.any() {
                break;
            }
        }
        Asks { rescan, end: false }.and(outcome::put(self, found, Phase::Running))
    }

    /// Counts `event`'s item when something arrived there and is gone now.
    fn count_gone(&mut self, event: &Event) {
        if event.flags & ARRIVED == 0
            || !event.path.starts_with(&self.matter)
            || self.fs.meta(&event.path).is_some()
        {
            return;
        }
        self.gone += 1;
        let total = GONE.fetch_add(1, Ordering::Relaxed) + 1;
        tracing::debug!(
            path = %event.path.display(), session = self.gone, total,
            "an event's item was gone when checked"
        );
    }

    /// Scans the whole matter: while the agent may run the first find, or a folder that can't
    /// be read, ends the session and the scan; once it has ended the scan is complete, each
    /// find is set aside and each folder it can't read is kept as stuck.
    pub(crate) fn full_scan(&mut self, phase: Phase) -> Asks {
        if phase == Phase::Ended && self.gone() > 0 {
            tracing::info!(
                matter = %self.matter.display(), session = self.gone(), total = gone_total(),
                "items the session's events reported were gone when checked"
            );
        }
        let found = self.scan(&self.matter, phase);
        outcome::put(self, found, phase)
    }

    /// Whether `path` (with path key `key`) is in `map`, still with its file ID.
    fn kept(&self, map: &HashMap<PathBuf, Id>, key: &Path, path: &Path) -> bool {
        map.get(key)
            .is_some_and(|id| self.fs.meta(path).is_some_and(|meta| meta.id == *id))
    }

    /// Adds what `event` brings to `items`, after `names::named` found no name in it. A
    /// `.git` at or above its path is found itself, there or not; its folder's `HEAD` is
    /// checked whatever happened; a folder that arrived is scanned, taking from `budget` the
    /// entries it lists, and a link that arrived is checked. Nothing at or below one of the
    /// launch's repositories is. Asks for a rescan when the events are incomplete or the path
    /// is outside the matter, and to end the session when the scan ran past the budget.
    fn event(&self, event: &Event, found: &mut Found, budget: &Cell<usize>) -> Asks {
        let items = &mut found.items;
        let rescan = Asks {
            rescan: true,
            end: false,
        };
        if event.flags & RESCAN != 0 {
            return rescan;
        }
        let Ok(rel) = event.path.strip_prefix(&self.matter) else {
            tracing::debug!(path = %event.path.display(), "an event outside the matter: rescan");
            return rescan;
        };
        let mut at = self.matter.clone();
        let mut key = path_key(&self.matter);
        for name in rel {
            at.push(name);
            key.push(name_key(name));
            if self.kept(&self.repos, &key, &at) || self.skip.contains(&key) {
                return Asks::default();
            }
            if is_git(name) {
                let kind = self.fs.meta(&at).map_or(Kind::Dir, |meta| meta.kind);
                items.push((at, GitItem::DotGit, kind));
                return Asks::default();
            }
        }
        items.extend(self.common_at(&key));
        if let Some(dir) = event
            .path
            .parent()
            .filter(|dir| dir.starts_with(&self.matter))
            && let Some(meta) = self.fs.meta(dir)
            && let Ok(entries) = self.fs.list(dir)
        {
            items.extend(self.head(dir, meta, &entries));
        }
        let Some(meta) = self.fs.meta(&event.path) else {
            return Asks::default();
        };
        if event.flags & ARRIVED == 0 {
            return Asks::default();
        }
        let mut asks = Asks::default();
        match meta.kind {
            Kind::Dir => asks.end = !self.scan_within(&event.path, budget, true, found),
            Kind::Link => items.extend(self.link(&event.path, meta.kind)),
            Kind::File | Kind::Other => {}
        }
        asks
    }

    /// The `commondir` path at or above path key `key`, when something new is there.
    fn common_at(&self, key: &Path) -> Option<Item> {
        let (path, launch) = self
            .common
            .iter()
            .find(|(common, _)| key.starts_with(common))
            .map(|(_, found)| found)?;
        let meta = self.fs.meta(path)?;
        (*launch != Some(meta.id)).then(|| (path.clone(), GitItem::CommonDirPart, meta.kind))
    }

    /// Folder `dir`'s `HEAD`, unless the folder is one of the launch's repositories, or a launch
    /// folder with a `HEAD` that is still not repository-shaped: the folder whole when it is new
    /// and repository-shaped, else the `HEAD` alone.
    fn head(&self, dir: &Path, meta: Meta, entries: &Entries) -> Option<Item> {
        let (head, kind) = head_of(entries)?;
        let key = path_key(dir);
        let shaped = repo_head(entries).is_some();
        if self.kept(&self.repos, &key, dir) || (!shaped && self.kept(&self.heads, &key, dir)) {
            return None;
        }
        let new = dir != self.matter && !self.all_old && !self.folders.contains(&meta.id);
        Some(if new && shaped {
            (dir.to_owned(), GitItem::BareRepo, Kind::Dir)
        } else {
            (dir.join(head), GitItem::BareRepoHead, kind)
        })
    }

    /// The link at `path`, when it leads out of the matter or nowhere and is not the launch's:
    /// judged by [`Fs::leads_out`], which follows no link, in both phases.
    fn link(&self, path: &Path, kind: Kind) -> Option<Item> {
        if self.kept(&self.links, &path_key(path), path) {
            return None;
        }
        self.fs
            .leads_out(path, &self.matter)
            .then(|| (path.to_owned(), GitItem::OutsideLink, kind))
    }

    /// Everything found in folder `top` and below: each `.git` not the launch's, each `HEAD`
    /// [`Watch::head`] takes, each link out, each new `commondir` path, and each folder that
    /// can't be read. While the agent may run it stops at the first; after, it is complete.
    fn scan(&self, top: &Path, phase: Phase) -> Found {
        let mut found = Found::default();
        let unbounded = Cell::new(usize::MAX);
        self.scan_within(top, &unbounded, phase == Phase::Running, &mut found);
        found
    }

    /// [`Watch::scan`] of `top` into `found`, listing at most `budget`'s entries, which it
    /// takes from it, and stopping at the first find when `first`. Returns false when it ran
    /// past the budget and stopped.
    fn scan_within(
        &self,
        top: &Path,
        budget: &Cell<usize>,
        first: bool,
        found: &mut Found,
    ) -> bool {
        let over = Cell::new(false);
        let (mut items, mut unread) = (Vec::new(), Vec::new());
        let mounts = RefCell::new(Vec::new());
        let stop = if first { Next::Stop } else { Next::Past };
        let mut folder = |dir: &Path, meta: Meta, entries: &Entries| {
            if self.new_volume(meta.id.0) {
                mounts.borrow_mut().push(dir.to_owned());
                return stop;
            }
            let Some(left) = budget.get().checked_sub(entries.len()) else {
                over.set(true);
                return Next::Stop;
            };
            budget.set(left);
            let below = self.folder(dir, meta, entries, &mut items);
            if first && !items.is_empty() {
                Next::Stop
            } else if below {
                Next::Below
            } else {
                Next::Past
            }
        };
        let mut unreadable = |dir: &Path, error: &io::Error| {
            unread.push((dir.to_owned(), error.to_string()));
            stop
        };
        let mut refused = |dir: &Path| {
            if self.new_refusal(dir) {
                mounts.borrow_mut().push(dir.to_owned());
                return stop;
            }
            Next::Past
        };
        let mut visit = Visit {
            skip: &|dir, _| self.passed(&path_key(dir), dir),
            folder: &mut folder,
            unreadable: &mut unreadable,
            refused: &mut refused,
            progress: &*self.progress,
        };
        walk_with(&self.fs, top, &mut visit);
        found.items.extend(items);
        found.unreadable.extend(unread);
        found.mounts.extend(mounts.into_inner());
        !over.get()
    }

    /// The next stretch of the 30 s pass (`check/sweep.rs`), while the agent may run, until
    /// `out_of_time` says so (asked before each folder) or the pass reaches its end. Returns
    /// what the finds ask (a find ends the session) and whether the pass reached its end; the
    /// next call goes on where this one stopped.
    pub(crate) fn sweep(&mut self, out_of_time: &dyn Fn() -> bool) -> (Asks, bool) {
        let mut sweep = std::mem::take(&mut self.sweep);
        let mut found = Found::default();
        let done = sweep::stretch(self, &mut sweep, out_of_time, &mut found);
        self.sweep = sweep;
        (outcome::put(self, found, Phase::Running), done)
    }

    /// The folders on volumes a scan never enters (network volumes, automounts) that the launch
    /// walk met under the folder.
    pub(crate) fn refused_at_launch(&self) -> Vec<PathBuf> {
        let volumes = self.base.volumes.as_ref();
        volumes.map_or_else(Vec::new, |v| {
            v.refused.iter().map(super::state::Bytes::path).collect()
        })
    }

    /// Whether a folder on device `dev` is on a volume the launch didn't meet under the folder.
    fn new_volume(&self, dev: u64) -> bool {
        self.volumes
            .as_ref()
            .is_some_and(|seen| seen.new_device(dev))
    }

    /// Whether `dir`, on a volume a scan never enters, wasn't one at launch.
    fn new_refusal(&self, dir: &Path) -> bool {
        self.volumes
            .as_ref()
            .is_some_and(|seen| seen.new_refusal(dir))
    }

    /// Whether a scan passes folder `dir` (path key `key`) by: a launch repository, or the
    /// volume set-aside folder.
    fn passed(&self, key: &Path, dir: &Path) -> bool {
        self.kept(&self.repos, key, dir) || self.skip.contains(key)
    }

    /// Adds what folder `dir` holds itself to `items`: its `HEAD` as [`Watch::head`] takes
    /// it, each `.git` not the launch's, each link out and each new `commondir` path. Returns
    /// whether to go below it: not when the folder is taken whole.
    fn folder(&self, dir: &Path, meta: Meta, entries: &Entries, items: &mut Vec<Item>) -> bool {
        if let Some(item) = self.head(dir, meta, entries) {
            let whole = item.1 == GitItem::BareRepo;
            items.push(item);
            if whole {
                return false;
            }
        }
        for (name, kind) in entries {
            let path = dir.join(name);
            let key = path_key(&path);
            if is_git(name) {
                if !self.kept(&self.repos, &key, &path) {
                    items.push((path, GitItem::DotGit, *kind));
                }
            } else if let Some(item) = self.common_at(&key).filter(|item| item.0 == path) {
                items.push(item);
            } else if *kind == Kind::Link {
                items.extend(self.link(&path, *kind));
            }
        }
        true
    }

    /// What the final scan would set aside, moving nothing: for a session a process of which
    /// outlived its kill. Lists at most [`SCAN_BUDGET`] entries: the agent may still run.
    pub(crate) fn finds(&self) -> Vec<PathBuf> {
        let mut found = Found::default();
        if !self.scan_within(&self.matter, &Cell::new(SCAN_BUDGET), false, &mut found) {
            tracing::warn!(
                budget = SCAN_BUDGET,
                "the list of what a session that outlived its kill left stopped at its budget"
            );
        }
        let here = |path: &PathBuf| self.fs.meta(path).is_some();
        found
            .items
            .into_iter()
            .map(|item| item.0)
            .filter(here)
            .collect()
    }
}
