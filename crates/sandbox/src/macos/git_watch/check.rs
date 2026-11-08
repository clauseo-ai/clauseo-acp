//! The watch's decisions: which events and which entries of a scan are set aside.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::aside::Aside;
use super::{
    ARRIVED, Asks, Event, Fs, Kind, Launch, Meta, Phase, RESCAN, is_git, is_head, repo_head,
};
use crate::policy::{name_key, path_key, under};
use crate::{DenialSink, GitItem};

/// One session's watch of one matter.
pub(crate) struct Watch<F> {
    fs: F,
    matter: PathBuf,
    aside: Aside,
    /// The launch's repositories, left alone while each keeps its file ID: its `.git`s and its
    /// bare repositories, by path key.
    repos: HashMap<PathBuf, (u64, u64)>,
    /// The launch's links leading out of the matter or nowhere, by path key, with their IDs.
    links: HashMap<PathBuf, (u64, u64)>,
    /// The file IDs of the matter's folders at the start.
    folders: HashSet<(u64, u64)>,
}

/// A folder's listing.
type Entries = [(OsString, Kind)];

/// One item to set aside.
type Item = (PathBuf, GitItem, Kind);

impl<F: Fs> Watch<F> {
    /// The watch of `matter` for `session`, setting aside into `set_aside`. The launch's items'
    /// file IDs are read now, and the matter is walked once for its folders and bare
    /// repositories, so call it before the agent starts.
    pub(crate) fn new(
        fs: F,
        matter: PathBuf,
        set_aside: PathBuf,
        session: String,
        launch: &Launch,
        notices: Arc<dyn DenialSink>,
    ) -> Watch<F> {
        let ids = |paths: &[PathBuf]| -> HashMap<PathBuf, (u64, u64)> {
            paths
                .iter()
                .filter_map(|path| Some((path_key(path), fs.meta(path)?.id)))
                .collect()
        };
        let repos = ids(&launch.git);
        let links = ids(&launch.links);
        let aside = Aside {
            matter: matter.clone(),
            folder: set_aside,
            session,
            notices,
            ended_by_us: false,
            count: 0,
        };
        let mut watch = Watch {
            fs,
            matter,
            aside,
            repos,
            links,
            folders: HashSet::new(),
        };
        let mut folders = HashSet::new();
        let mut bare = Vec::new();
        walk(
            &watch.fs,
            &watch.matter,
            &|_, _| false,
            &mut |dir, meta, entries| {
                folders.insert(meta.id);
                if repo_head(entries).is_some() {
                    bare.push((path_key(dir), meta.id));
                }
                true
            },
        );
        watch.folders = folders;
        watch.repos.extend(bare);
        watch
    }

    /// The matter.
    pub(crate) fn matter(&self) -> &Path {
        &self.matter
    }

    /// Makes the set-aside folder, before the agent starts.
    pub(crate) fn make_set_aside(&self) -> std::io::Result<()> {
        self.fs.make_dirs(&self.aside.folder)
    }

    /// The runner ended the session for a failed move.
    pub(crate) fn ended_session(&mut self) {
        self.aside.ended_by_us = true;
    }

    /// Checks one batch of events.
    pub(crate) fn events(&mut self, events: &[Event], phase: Phase) -> Asks {
        let mut items = Vec::new();
        let mut rescan = false;
        for event in events {
            rescan |= self.event(event, &mut items);
        }
        Asks { rescan, end: false }.and(self.put(items, phase))
    }

    /// Scans the whole matter.
    pub(crate) fn full_scan(&mut self, phase: Phase) -> Asks {
        let items = self.scan(&self.matter);
        self.put(items, phase)
    }

    fn put(&mut self, items: Vec<Item>, phase: Phase) -> Asks {
        let mut asks = Asks::default();
        for (path, what, kind) in items {
            let local = path.parent().is_some_and(|dir| self.fs.local(dir));
            if local && self.fs.meta(&path).is_some() {
                asks = asks.and(self.aside.put(&self.fs, &path, what, kind, phase));
            }
        }
        asks
    }

    /// Whether `path` is one of the launch's repositories, still with its file ID.
    fn launch_repo(&self, key: &Path, path: &Path) -> bool {
        self.repos
            .get(key)
            .is_some_and(|id| self.fs.meta(path).is_some_and(|meta| meta.id == *id))
    }

    /// Adds what `event` brings to `items`; true when it asks for a rescan. A `.git` at or above
    /// its path is set aside itself; a folder that arrived is scanned; a `HEAD` or a link that
    /// arrived is checked. Nothing at or below one of the launch's repositories is.
    fn event(&self, event: &Event, items: &mut Vec<Item>) -> bool {
        if event.flags & RESCAN != 0 {
            return true;
        }
        let Ok(rel) = event.path.strip_prefix(&self.matter) else {
            return false;
        };
        let mut at = self.matter.clone();
        let mut key = path_key(&self.matter);
        for name in rel {
            at.push(name);
            key.push(name_key(name));
            if self.launch_repo(&key, &at) {
                return false;
            }
            if is_git(name) {
                if let Some(meta) = self.fs.meta(&at) {
                    items.push((at, GitItem::DotGit, meta.kind));
                }
                return false;
            }
        }
        let Some(meta) = self.fs.meta(&event.path) else {
            return false;
        };
        if event.flags & ARRIVED == 0 {
            return false;
        }
        let head = event.path.file_name().is_some_and(is_head);
        match meta.kind {
            Kind::Dir => items.extend(self.scan(&event.path)),
            Kind::File | Kind::Link if head => items.push(self.head(&event.path, meta.kind)),
            Kind::Link => items.extend(self.link(&event.path, meta.kind)),
            Kind::File | Kind::Other => {}
        }
        false
    }

    /// A new `HEAD` file or link: its folder when that is new and repository-shaped, else the file.
    fn head(&self, path: &Path, kind: Kind) -> Item {
        let new_repo = path
            .parent()
            .filter(|dir| *dir != self.matter)
            .filter(|dir| {
                self.fs
                    .meta(dir)
                    .is_some_and(|meta| !self.folders.contains(&meta.id))
                    && self
                        .fs
                        .list(dir)
                        .is_ok_and(|entries| repo_head(&entries).is_some())
            });
        match new_repo {
            Some(dir) => (dir.to_owned(), GitItem::BareRepo, Kind::Dir),
            None => (path.to_owned(), GitItem::BareRepoHead, kind),
        }
    }

    /// The link at `path`, when it leads out of the matter or nowhere and is not the launch's.
    fn link(&self, path: &Path, kind: Kind) -> Option<Item> {
        let launch = self.links.get(&path_key(path));
        if launch.is_some_and(|id| self.fs.meta(path).is_some_and(|meta| meta.id == *id)) {
            return None;
        }
        let inside = self
            .fs
            .resolve(path)
            .is_some_and(|target| under(&target, &self.matter));
        (!inside).then(|| (path.to_owned(), GitItem::OutsideLink, kind))
    }

    /// Everything to set aside in folder `top` and below: each `.git` not the launch's, each
    /// repository-shaped folder not the launch's (whole when new, else its `HEAD`), and each
    /// link out.
    fn scan(&self, top: &Path) -> Vec<Item> {
        let mut items = Vec::new();
        let skip = |dir: &Path, _: Meta| self.launch_repo(&path_key(dir), dir);
        walk(&self.fs, top, &skip, &mut |dir, meta, entries| {
            if let Some(head) = repo_head(entries) {
                if dir != self.matter && !self.folders.contains(&meta.id) {
                    items.push((dir.to_owned(), GitItem::BareRepo, Kind::Dir));
                    return false;
                }
                let kind = self.fs.meta(&dir.join(head)).map_or(Kind::File, |m| m.kind);
                items.push((dir.join(head), GitItem::BareRepoHead, kind));
            }
            for (name, kind) in entries {
                let path = dir.join(name);
                if is_git(name) {
                    if !self.launch_repo(&path_key(&path), &path) {
                        items.push((path, GitItem::DotGit, *kind));
                    }
                } else if *kind == Kind::Link {
                    items.extend(self.link(&path, *kind));
                }
            }
            true
        });
        items
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
