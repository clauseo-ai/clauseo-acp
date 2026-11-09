//! The live watch's half of the set-aside: a git repository or an outbound link made while the
//! agent runs, moved aside at once through the same move, manifest and lock as the after-session
//! pass (`moves::set_aside`). The outer runner's watch (`runner/git_watch.rs`) hands over the
//! paths `ReadDirectoryChangesW` reported, or asks for a full rescan when the watch overflowed.
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
//! first item found ends the agent's job, so nothing alive races the moves that follow. A
//! reported record whose own name is `.git`, or `HEAD` (outside every `.git`, and not in a
//! launch `.git`), ends the session at once, whether or not it is still there by the check, so
//! an agent can't keep short-lived `.git`s coming and going ([`Watched::changed`]); those are
//! moved first, before any folder reported is scanned. An unknown git folder a scan finds ends
//! the session before anything in it is read (`check.rs`). Each
//! scan moves what it finds as it finds it (`scan::Found`), never after the whole tree, and a
//! scan that runs past [`SCAN_BUDGET`] ends the session too. In a folder made or moved in, that
//! scan stops there, and the folder goes whole only when a pool user owns it, read from the
//! handle the move renames; one whose owner can't be read stays (`check::Run::below`); a full
//! rescan, or one at the stop with the job empty, walks on. An item gone before it could be
//! moved, or a folder reported renamed or moved in and gone before it could be scanned, is listed
//! ([`Live::gone`]), so the watch can end a session whose agent keeps renaming one back and
//! forth. A folder that becomes a link (`FILE_ACTION_MODIFIED` with the reparse
//! attribute: a junction set on an empty folder, `mklink /J`'s two steps) is checked as a link
//! made ([`Watched::relinked`]). What git reads from a folder a launch `commondir` names
//! (`scan::Kept::Common`) is moved when made, as a `.git` is.
//!
//! Every read here goes through handles, never by a full path (rule 8, `scan/reach.rs`): the
//! watched folder is opened as itself, then each folder on the way relative to the one before,
//! as itself, and a link on the way stops the read, never followed. An item past a folder that is
//! a link now is gone from the watched folder, never moved through the link. A link's target is
//! read from the link's own handle, and told inside the watched folder by identity, never by
//! spelling (`scan/locate.rs`): a network path, another volume or a `..` past the folder leads
//! out, and nothing of a network path is opened. The move itself still opens the item's folder by
//! its path and checks its final path (`Volume::move_into`).
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
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use clauseo_protocol::MatterId;

use self::check::Run;
use self::keys::{ends_in_git, holds, key_of, repo_part};
use super::files::{FileId, Place, Record};
use super::names::Key;
use super::repo_names::{COMMON_NAMES, is_git, is_head, opens_as};
use super::run::Volume;
use super::scan::{COMMON_KEY, Item, Kept, Kind, Last, Missed, Now, locate, reach, same_key};
use super::scan::{scan, scan_visiting};
use super::{Entry, SetAsideFailure};
use crate::SetAsideWhat;

mod check;
mod keys;

/// How long one scan may run while the agent could still be changing the tree: past it, the
/// session ends, and the scan carries on with nothing alive to race it.
pub(crate) const SCAN_BUDGET: Duration = Duration::from_secs(2);

/// One item the watch moved: its entry, its path below the watched folder, what it was, and
/// whether a pool user (the agent) owned it.
#[derive(Debug, Clone)]
pub(crate) struct Moved {
    pub(crate) entry: Entry,
    pub(crate) path: PathBuf,
    pub(crate) what: SetAsideWhat,
    /// Whether the owner read from the handle the move renamed is a pool SID the machine's record
    /// names: the agent made it, and the lawyer's notice has no step to put it back. One whose
    /// owner couldn't be read counts as the agent's too: the agent can deny the lawyer read
    /// access on its own `.git`, and that must not earn it the notice that says to move it back.
    pub(crate) by_agent: bool,
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
    /// Each item, by its full path, found and not moved because the session's end couldn't be
    /// confirmed ([`Hooks::end`] said the agent's job isn't empty yet): the check after the
    /// session moves it.
    pub(crate) left: Vec<PathBuf>,
    /// Each folder reported made just now (`fresh`, by its creation time) and gone before it
    /// could be scanned, by its path below the watched folder: it counts as gone only when its
    /// file ID left another place (`runner/git_watch/pairs.rs`).
    pub(crate) unpaired: Vec<PathBuf>,
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
    /// Ends the session now, the first time; every later call ends nothing more. Whether the
    /// agent's job is truly empty: only then does the check move anything.
    fn end(&self, why: Ending) -> bool;
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
        Watched::from_place(volume, root, Place::new(data_dir, matter))
    }

    /// [`Watched::new`] for `matter`'s notes folder `root`, from its record (`Place::notes`).
    pub(super) fn new_notes(
        volume: &impl Volume,
        root: &Path,
        data_dir: &Path,
        matter: &MatterId,
    ) -> Watched {
        Watched::from_place(volume, root, Place::notes(data_dir, matter))
    }

    fn from_place(volume: &impl Volume, root: &Path, place: Place) -> Watched {
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
    #[cfg(test)]
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

    /// Checks each path the watch reported made or renamed, relative to the root. First each
    /// name hit ([`Watched::named`]): one ends the session at once, gone or not, and each is
    /// moved; then each `.git` part, link and `HEAD` below a launch git folder; only then the
    /// folders reported, scanned below.
    #[cfg(test)]
    pub(super) fn changed(
        &self,
        volume: &impl Volume,
        paths: &[PathBuf],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        self.changed_with_folders(volume, paths, (&[], &[]), now, hooks)
    }

    /// [`Watched::changed`], told which of `paths` were folders renamed or moved in
    /// (`git_watch/records.rs`): one gone by its check (nothing at its name, or past a link
    /// now) is listed in [`Live::gone`], as a gone `.git` is, so an agent can't move a folder
    /// holding a `.git` in and back out before it is scanned, again and again. A folder in
    /// `fresh` (made just now, by its creation time) gone so is listed in [`Live::unpaired`]
    /// instead, for the watch to pair by its file ID.
    pub(super) fn changed_with_folders(
        &self,
        volume: &impl Volume,
        paths: &[PathBuf],
        (folders, fresh): (&[PathBuf], &[PathBuf]),
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        let folders: Vec<Key> = folders.iter().filter_map(|p| key_of(p)).collect();
        let fresh: Vec<Key> = fresh.iter().filter_map(|p| key_of(p)).collect();
        let mut live = Live::default();
        let mut run = Run::new(self, volume, now, hooks, &mut live);
        let (mut hits, mut rest) = (Vec::new(), Vec::new());
        for path in paths {
            let Some(key) = key_of(path) else {
                tracing::debug!("the watch reported a path that isn't plain; passed over");
                continue;
            };
            if self.named(volume, &key) {
                hits.push(key);
            } else {
                rest.push(key);
            }
        }
        if !hits.is_empty() {
            run.end_found();
        }
        for key in &hits {
            run.check(key, SetAsideWhat::GitRepository);
        }
        let (mut scanned, mut gone, mut unpaired) = (Vec::new(), Vec::<PathBuf>::new(), Vec::new());
        for key in rest {
            let repo = key.iter().position(|part| is_git(part)).map(|at| at + 1);
            if let Some(end) = repo.or_else(|| self.common_at(&key)) {
                let part = key.get(..end).unwrap_or_default();
                run.check(part, SetAsideWhat::GitRepository);
                continue;
            }
            match reach(volume, &self.root, &key) {
                Ok((folder, Some((_, Kind::Link)))) => {
                    let out = self.link_leads_out(volume, &folder, &key);
                    drop(folder);
                    if out {
                        run.check(&key, SetAsideWhat::LinkOut);
                    }
                }
                Ok((_, Some((_, Kind::Folder)))) => scanned.push(key),
                Ok((_, None)) | Err(Missed::Gone | Missed::Link) if folders.contains(&key) => {
                    tracing::debug!("a folder reported went before it could be scanned");
                    gone.push(key.iter().collect());
                }
                Ok((_, None)) | Err(Missed::Gone | Missed::Link) if fresh.contains(&key) => {
                    unpaired.push(key.iter().collect());
                }
                Ok((_, Some((_, Kind::Other)) | None)) | Err(_) => {}
            }
        }
        for key in &scanned {
            run.below(key);
        }
        live.gone.extend(gone);
        live.unpaired.extend(unpaired);
        live
    }

    /// Whether the reported `key` is a name hit, which ends the session whether or not anything
    /// is there now: its own name opens as `.git`, outside every `.git` part; or it is `HEAD`
    /// outside every `.git`, unless its folder is there and is a launch `.git` by file ID (the
    /// lawyer's git renames `HEAD.lock` to `HEAD` in a `.git` the lawyer renamed) or it is a
    /// folder, which git never reads as a `HEAD` (scanned as any folder made is). Below a `.git`
    /// part (the lawyer's git working in a known one) nothing is a hit.
    fn named(&self, volume: &impl Volume, key: &[OsString]) -> bool {
        let Some((last, parents)) = key.split_last() else {
            return false;
        };
        if parents.iter().any(|part| is_git(part)) {
            return false;
        }
        if is_git(last) {
            return true;
        }
        if !is_head(last) {
            return false;
        }
        match reach(volume, &self.root, key) {
            Ok((_, Some((_, Kind::Folder)))) => false,
            Ok((folder, _)) => !volume
                .folder_id(&folder)
                .is_ok_and(|id| self.git_folder(id)),
            Err(_) => true,
        }
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
            let out = match reach(volume, &self.root, &key) {
                Ok((folder, Some((_, Kind::Link)))) => self.link_leads_out(volume, &folder, &key),
                Ok(_) | Err(_) => false,
            };
            if out {
                run.check(&key, SetAsideWhat::LinkOut);
            }
        }
        live
    }

    /// Scans the whole folder and moves, as it finds it, what the watch moves that it doesn't
    /// know: after an overflow, and at the session's end.
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
            SetAsideWhat::UncheckedFolder => false,
        }
    }

    /// Whether `id` is a launch `.git`'s: one the lawyer renamed is still denied, and a `HEAD`
    /// in it is that `.git`'s own.
    fn git_folder(&self, id: FileId) -> bool {
        self.known
            .iter()
            .any(|(key, at)| *at == id && ends_in_git(key))
    }

    /// Whether the item at `key` is a link that leads outside the watched folder, or one whose
    /// target can't be read, or isn't a link there now, reached through handles ([`reach`]):
    /// anything but a link read and found to lead within. A folder on the way that is a link
    /// now counts as leading out (the move then finds the item gone, past it).
    fn leads_out(&self, volume: &impl Volume, key: &[OsString]) -> bool {
        match reach(volume, &self.root, key) {
            Ok((folder, Some((_, Kind::Link)))) => self.link_leads_out(volume, &folder, key),
            Ok(_) | Err(_) => true,
        }
    }

    /// Whether the link at `key`, in the open `folder`, leads outside the watched folder, or its
    /// target can't be read. The target is read from the link's own handle, never followed
    /// ([`Volume::link_target`]), worked out from its words relative to the link's folder, and
    /// told inside by identity ([`locate`]): never by spelling, and a network path is never
    /// opened. A link out is moved before anything follows it.
    fn link_leads_out<V: Volume>(&self, volume: &V, folder: &V::Folder, key: &[OsString]) -> bool {
        let Some((name, parents)) = key.split_last() else {
            return true;
        };
        let Ok(target) = volume.link_target(folder, name) else {
            return true;
        };
        locate(
            volume,
            &self.root,
            (&self.path(parents), &target),
            Last::Must,
        )
        .is_none()
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
