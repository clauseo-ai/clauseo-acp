//! The watch's decisions: which events and which entries of a scan are found, and, once the
//! session has ended, set aside.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::aside::{Aside, Stuck};
use super::state::{Baseline, Id, Known};
use super::{
    ARRIVED, Asks, Event, Fs, Kind, Launch, Meta, Phase, RESCAN, head_of, is_git, repo_head,
};
use crate::GitItem;
use crate::policy::{name_key, path_key, under};

mod sweep;

pub(crate) use sweep::Sweep;

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
}

/// A folder's listing.
type Entries = [(OsString, Kind)];

/// One item to set aside.
type Item = (PathBuf, GitItem, Kind);

impl<F: Fs> Watch<F> {
    /// The watch of `matter`, setting aside through `aside`. The launch's items' file IDs are
    /// read now, and the matter is walked once for its folders, repositories and `HEAD`
    /// folders, so call it before the agent starts.
    pub(crate) fn new(fs: F, matter: PathBuf, aside: Aside, launch: &Launch) -> Watch<F> {
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
        walk(
            &fs,
            &matter,
            &|dir, _| skip.contains(&path_key(dir)),
            &mut |dir, meta, entries| {
                folders.insert(meta.id);
                if repo_head(entries).is_some() {
                    base.repos.push(Known::new(dir, meta.id));
                    repo_dirs.insert(dir.to_owned());
                } else if head_of(entries).is_some() {
                    base.heads.push(Known::new(dir, meta.id));
                }
                // The 30 s pass, as a scan, never enters a launch repository.
                if !dir.ancestors().any(|above| repo_dirs.contains(above)) {
                    sweep.add(dir, Some(meta.id), fs.dated(dir).map(|(_, stamp)| stamp));
                }
                true
            },
        );
        let mut watch = Watch::resume(fs, matter, aside, base);
        watch.folders = folders;
        watch.all_old = false;
        watch.sweep = sweep;
        watch
    }

    /// The watch of a session from its recorded `base`, for its final scan: every folder
    /// counts as old, so a repository-shaped folder loses only its `HEAD`.
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

    /// Checks one batch of events, while the agent may run: finds end the session.
    pub(crate) fn events(&mut self, events: &[Event]) -> Asks {
        let mut items = Vec::new();
        let mut rescan = false;
        for event in events {
            rescan |= self.event(event, &mut items);
        }
        Asks { rescan, end: false }.and(self.put(items, Phase::Running))
    }

    /// Scans the whole matter: while the agent may run a find ends the session; once it has
    /// ended, each find is set aside.
    pub(crate) fn full_scan(&mut self, phase: Phase) -> Asks {
        let items = self.scan(&self.matter);
        self.put(items, phase)
    }

    fn put(&mut self, items: Vec<Item>, phase: Phase) -> Asks {
        let mut asks = Asks::default();
        for (path, what, kind) in items {
            let local = path.parent().is_some_and(|dir| self.fs.local(dir));
            if !local || self.fs.meta(&path).is_none() {
                continue;
            }
            match phase {
                Phase::Running => {
                    tracing::warn!(
                        path = %path.display(), ?what,
                        "the git watch found what an agent left in the matter; ending the session"
                    );
                    asks.end = true;
                }
                Phase::Ended => self.aside.put(&self.fs, &path, what, kind),
            }
        }
        asks
    }

    /// Whether `path` (with path key `key`) is in `map`, still with its file ID.
    fn kept(&self, map: &HashMap<PathBuf, Id>, key: &Path, path: &Path) -> bool {
        map.get(key)
            .is_some_and(|id| self.fs.meta(path).is_some_and(|meta| meta.id == *id))
    }

    /// Adds what `event` brings to `items`; true when it asks for a rescan. A `.git` at or
    /// above its path is found itself; its folder's `HEAD` is checked whatever happened; a
    /// folder that arrived is scanned and a link that arrived is checked. Nothing at or below
    /// one of the launch's repositories is.
    fn event(&self, event: &Event, items: &mut Vec<Item>) -> bool {
        if event.flags & RESCAN != 0 {
            return true;
        }
        let Ok(rel) = event.path.strip_prefix(&self.matter) else {
            tracing::debug!(path = %event.path.display(), "an event outside the matter: rescan");
            return true;
        };
        let mut at = self.matter.clone();
        let mut key = path_key(&self.matter);
        for name in rel {
            at.push(name);
            key.push(name_key(name));
            if self.kept(&self.repos, &key, &at) || self.skip.contains(&key) {
                return false;
            }
            if is_git(name) {
                if let Some(meta) = self.fs.meta(&at) {
                    items.push((at, GitItem::DotGit, meta.kind));
                }
                return false;
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
            return false;
        };
        if event.flags & ARRIVED == 0 {
            return false;
        }
        match meta.kind {
            Kind::Dir => items.extend(self.scan(&event.path)),
            Kind::Link => items.extend(self.link(&event.path, meta.kind)),
            Kind::File | Kind::Other => {}
        }
        false
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

    /// The link at `path`, when it leads out of the matter or nowhere and is not the launch's.
    fn link(&self, path: &Path, kind: Kind) -> Option<Item> {
        if self.kept(&self.links, &path_key(path), path) {
            return None;
        }
        let inside = self
            .fs
            .resolve(path)
            .is_some_and(|target| under(&target, &self.matter));
        (!inside).then(|| (path.to_owned(), GitItem::OutsideLink, kind))
    }

    /// Everything found in folder `top` and below: each `.git` not the launch's, each `HEAD`
    /// [`Watch::head`] takes, each link out, and each new `commondir` path.
    fn scan(&self, top: &Path) -> Vec<Item> {
        let mut items = Vec::new();
        let skip = |dir: &Path, _: Meta| self.passed(&path_key(dir), dir);
        walk(&self.fs, top, &skip, &mut |dir, meta, entries| {
            self.folder(dir, meta, entries, &mut items)
        });
        items
    }

    /// The next stretch of the 30 s pass (`check/sweep.rs`), while the agent may run, until
    /// `out_of_time` says so (asked before each folder) or the pass reaches its end. Returns
    /// what the finds ask (a find ends the session) and whether the pass reached its end; the
    /// next call goes on where this one stopped.
    pub(crate) fn sweep(&mut self, out_of_time: &dyn Fn() -> bool) -> (Asks, bool) {
        let mut sweep = std::mem::take(&mut self.sweep);
        let mut items = Vec::new();
        let done = sweep::stretch(self, &mut sweep, out_of_time, &mut items);
        self.sweep = sweep;
        (self.put(items, Phase::Running), done)
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
}

/// Visits folder `top` and every folder below it with its file ID and listing, never through
/// a link, never into a `.git`, a folder `skip` names or a network volume. `visit` says whether
/// to go below the folder.
fn walk<F: Fs>(
    fs: &F,
    top: &Path,
    skip: &dyn Fn(&Path, Meta) -> bool,
    visit: &mut dyn FnMut(&Path, Meta, &Entries) -> bool,
) {
    let mut folders: Vec<(PathBuf, Option<u64>)> = vec![(top.to_owned(), None)];
    while let Some((dir, above)) = folders.pop() {
        let Some(meta) = fs.meta(&dir).filter(|meta| meta.kind == Kind::Dir) else {
            continue;
        };
        if skip(&dir, meta) {
            continue;
        }
        let volume = meta.id.0;
        if above != Some(volume) && !fs.local(&dir) {
            tracing::info!(dir = %dir.display(), "the git watch skips a network volume");
            continue;
        }
        let entries = match fs.list(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(dir = %dir.display(), %error, "the git watch can't list a folder");
                continue;
            }
        };
        if !visit(&dir, meta, &entries) {
            continue;
        }
        for (name, kind) in entries {
            if kind == Kind::Dir && !is_git(&name) {
                folders.push((dir.join(name), Some(volume)));
            }
        }
    }
}
