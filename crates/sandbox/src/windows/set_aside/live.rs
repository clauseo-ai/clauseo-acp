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
use std::time::SystemTime;

use clauseo_protocol::MatterId;

use super::files::{FileId, Place};
use super::moves::set_aside;
use super::names::Key;
use super::repo_names::{is_git, is_head};
use super::run::{Refused, Volume};
use super::scan::{Item, Kept, Now, Scan, same_key, scan, scan_part};
use super::{Entry, Reason, SetAsideFailure};
use crate::SetAsideWhat;

/// One item the watch moved: its entry, its path below the matter's folder, and what it was.
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
}

/// One matter's watched folder, its set-aside folder, and what it knew at launch.
#[derive(Debug)]
pub(crate) struct Watched {
    root: PathBuf,
    place: Place,
    /// Each `.git` and `HEAD` there was at launch.
    known: Vec<(Key, FileId)>,
    /// Each link there was at launch.
    links: Vec<(Key, FileId)>,
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
        let (known, links) = match record {
            Ok(Some(record)) if record.holds_heads() => (
                record
                    .names
                    .into_iter()
                    .filter_map(|(key, id)| Some((key, id?)))
                    .filter(|(key, _)| repo_part(key))
                    .collect(),
                record.links.into_iter().collect(),
            ),
            Ok(_) | Err(_) => {
                tracing::warn!("no set-aside record of the launch's git parts; scanning instead");
                let found = scan(volume, root);
                let at = |item: &Item| match item.now {
                    Now::At(id) => Some((item.key.clone(), id)),
                    Now::Absent | Now::Shut | Now::Failed(_) => None,
                };
                let (links, names): (Vec<&Item>, Vec<&Item>) =
                    found.items.iter().partition(|item| item.kept == Kept::Link);
                let known = names
                    .into_iter()
                    .filter(|i| repo_part(&i.key))
                    .filter_map(at);
                (known.collect(), links.into_iter().filter_map(at).collect())
            }
        };
        Watched {
            root: root.to_owned(),
            place,
            known,
            links,
        }
    }

    /// Checks each path the watch reported, relative to the root.
    pub(super) fn changed(&self, volume: &impl Volume, paths: &[PathBuf], now: SystemTime) -> Live {
        let mut live = Live::default();
        for path in paths {
            let Some(key) = key_of(path) else {
                tracing::debug!("the watch reported a path that isn't plain; passed over");
                continue;
            };
            let full = self.path(&key);
            if let Some(at) = key.iter().position(|part| is_git(part)) {
                let git = key.get(..=at).unwrap_or_default();
                self.check(volume, git, SetAsideWhat::GitRepository, now, &mut live);
                continue;
            }
            let Ok(meta) = std::fs::symlink_metadata(&full) else {
                continue;
            };
            let link = meta.file_type().is_symlink();
            let head = !meta.is_dir() && key.last().is_some_and(|name| is_head(name));
            if link && self.leads_out(&full) {
                self.check(volume, &key, SetAsideWhat::LinkOut, now, &mut live);
            } else if meta.is_dir() && !link {
                self.below(volume, &key, now, &mut live);
            } else if head || (link && key.last().is_some_and(|name| is_head(name))) {
                let folder = full.parent().map(|folder| volume.file_id(folder));
                if !matches!(folder, Some(Ok(Some(id))) if self.git_folder(id)) {
                    self.check(volume, &key, SetAsideWhat::GitRepository, now, &mut live);
                }
            }
        }
        live
    }

    /// Scans the whole matter and moves what the watch moves that it doesn't know: after an
    /// overflow, a watch armed again, or every few seconds while the watch can't be armed.
    pub(super) fn rescan(&self, volume: &impl Volume, now: SystemTime) -> Live {
        let mut live = Live::default();
        self.sweep(volume, &scan(volume, &self.root), false, now, &mut live);
        live
    }

    /// Tries again what a check couldn't move, once the session ended.
    pub(super) fn retry(&self, volume: &impl Volume, failed: &[Failed], now: SystemTime) -> Live {
        let mut live = Live::default();
        for each in failed {
            match each.path.strip_prefix(&self.root).ok().and_then(key_of) {
                Some(key) => self.check(volume, &key, each.what, now, &mut live),
                None => live.failed.push(Failed {
                    failure: SetAsideFailure::Changed,
                    ..each.clone()
                }),
            }
        }
        live
    }

    /// Scans below the folder `key`, made or moved in. A folder gone is nothing to scan.
    fn below(&self, volume: &impl Volume, key: &[OsString], now: SystemTime, live: &mut Live) {
        match scan_part(volume, &self.root, key, true) {
            Ok(Some(found)) => self.sweep(volume, &found, true, now, live),
            Ok(None) => {}
            Err(error) => {
                let error = error.kind();
                tracing::debug!(%error, "a reported folder the watch can't scan");
            }
        }
    }

    /// Moves what `found` holds that the watch moves and doesn't know: in a folder made or moved
    /// in (`new`), each bare repository's whole folder first. What is below a `.git` is left to
    /// that one: the known one's tree is denied, and an unknown one goes whole.
    fn sweep(
        &self,
        volume: &impl Volume,
        found: &Scan,
        new: bool,
        now: SystemTime,
        live: &mut Live,
    ) {
        let below_git = |key: &Key| {
            let parents = key.get(..key.len().saturating_sub(1)).unwrap_or_default();
            parents.iter().any(|part| is_git(part))
        };
        let in_git = |id: Option<FileId>| id.is_some_and(|id| self.git_folder(id));
        for (folder, _) in found.bare.iter().filter(|(_, id)| new && !in_git(*id)) {
            let head = found.items.iter().find(|item| {
                item.kept == Kept::Head
                    && item.key.len() == folder.len() + 1
                    && item.key.starts_with(folder)
            });
            let known = head
                .is_some_and(|head| matches!(head.now, Now::At(id) if self.knows(&head.key, id)));
            if !known && !below_git(folder) {
                let item = self.item(folder, Now::Shut);
                self.move_one(volume, &item, SetAsideWhat::GitRepository, now, live);
            }
        }
        for item in found.items.iter().filter(|item| !below_git(&item.key)) {
            let what = match item.kept {
                Kept::Link if self.leads_out(&item.path) => SetAsideWhat::LinkOut,
                Kept::Head if in_git(item.through.last().copied()) => continue,
                Kept::Head => SetAsideWhat::GitRepository,
                Kept::Root | Kept::Nested if ends_in_git(&item.key) => SetAsideWhat::GitRepository,
                Kept::Root | Kept::Nested | Kept::Link => continue,
            };
            match item.now {
                Now::Absent => {}
                Now::At(id) if self.knows_as(&item.key, id, what) => {}
                Now::At(_) | Now::Shut | Now::Failed(_) => {
                    self.move_one(volume, item, what, now, live);
                }
            }
        }
    }

    /// Moves the item at `key` as `what` unless nothing is there or the watch knows it there.
    fn check(
        &self,
        volume: &impl Volume,
        key: &[OsString],
        what: SetAsideWhat,
        now: SystemTime,
        live: &mut Live,
    ) {
        let now_at = match volume.file_id(&self.path(key)) {
            Ok(None) => return,
            Ok(Some(id)) if self.knows_as(key, id, what) => return,
            Ok(Some(id)) => Now::At(id),
            Err(_) => Now::Shut,
        };
        self.move_one(volume, &self.item(key, now_at), what, now, live);
    }

    /// Whether the record holds the item at `key` with the file ID `id`, as a link for
    /// [`SetAsideWhat::LinkOut`], else as a `.git` or `HEAD`.
    fn knows_as(&self, key: &[OsString], id: FileId, what: SetAsideWhat) -> bool {
        match what {
            SetAsideWhat::LinkOut => holds(&self.links, key, Some(id)),
            SetAsideWhat::GitRepository => self.knows(key, id),
        }
    }

    /// Whether `id` is a launch `.git`'s: one the lawyer renamed is still denied, and a `HEAD`
    /// in it is that `.git`'s own.
    fn git_folder(&self, id: FileId) -> bool {
        self.known
            .iter()
            .any(|(key, at)| *at == id && ends_in_git(key))
    }

    /// Whether the record holds a `.git` or `HEAD` at `key` with the file ID `id`.
    fn knows(&self, key: &[OsString], id: FileId) -> bool {
        holds(&self.known, key, Some(id))
    }

    /// Whether the link at `link` leads outside the matter's folder, or its target can't be
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

    /// Moves `item` as `what` under the matter's lock into this pass's folder at its own path,
    /// as the after-session pass does. One gone meanwhile is passed over: a move of it within
    /// the matter is reported on its own.
    fn move_one(
        &self,
        volume: &impl Volume,
        item: &Item,
        what: SetAsideWhat,
        now: SystemTime,
        live: &mut Live,
    ) {
        let seen = match what {
            SetAsideWhat::LinkOut => holds(&self.links, &item.key, None),
            SetAsideWhat::GitRepository => holds(&self.known, &item.key, None),
        };
        let reason = if seen { Reason::Changed } else { Reason::New };
        let moved = self
            .place
            .lock()
            .map_err(|failure| Refused::at(failure, &item.path))
            .and_then(|held| {
                let moved = set_aside(volume, &self.place, &item.path, &item.within, reason, now);
                drop(held);
                moved
            });
        match moved {
            Ok(entry) => live.moved.push(Moved {
                entry,
                path: item.key.iter().collect(),
                what,
            }),
            Err(refused) if refused.failure == SetAsideFailure::Gone => {
                tracing::debug!("an item went before the watch could set it aside");
            }
            Err(refused) => live.failed.push(Failed {
                path: item.path.clone(),
                failure: refused.failure,
                what,
            }),
        }
    }

    /// The full path of `key`.
    fn path(&self, key: &[OsString]) -> PathBuf {
        key.iter()
            .fold(self.root.clone(), |path, part| path.join(part))
    }

    /// An item at `key` for [`Watched::move_one`].
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

/// `path` as text parts, its `..` and `.` worked out by their words, with a verbatim prefix
/// (`\\?\C:\`, `\\?\UNC\`, `\??\`) read as the plain one, for comparing a link's target.
fn plain(path: &Path) -> Vec<String> {
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
