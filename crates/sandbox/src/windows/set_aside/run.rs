//! The set-aside's passes over a [`Volume`]: what a launch records and checks, what the
//! after-session pass checks, the walk's single move, the list and the restore. No Windows
//! call, so it builds and is tested on every OS.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use clauseo_protocol::MatterId;

use super::check::{Aside, check, snapshot};
use super::files::{FileId, Place, Record};
use super::moves::{hand_over, load, set_aside};
use super::repo_names::is_git;
use super::scan::{Listed, scan};
use super::{Checked, Entry, Reason, SetAsideFailure};

/// The file-system calls the set-aside makes: NTFS on Windows (`ntfs.rs`), and the tests'
/// stand-in elsewhere.
pub(super) trait Volume {
    /// A folder the scan opened, as itself, to probe and list.
    type Folder;

    /// Opens the matter's folder `root` as itself, to list it; a link there, or a final path
    /// other than `root`, is an error.
    fn open_root(&self, root: &Path) -> io::Result<Self::Folder>;

    /// Opens the folder `name` in `folder` relative to it, as itself, to list it; one that is
    /// a link now is an error.
    fn open_folder(&self, folder: &Self::Folder, name: &OsStr) -> io::Result<Self::Folder>;

    /// The file ID of `folder` itself, read from what the scan opened.
    fn folder_id(&self, folder: &Self::Folder) -> io::Result<FileId>;

    /// The names in `folder`, without `.` and `..`, and what each is.
    fn list(&self, folder: &Self::Folder) -> io::Result<Vec<Listed>>;

    /// The file ID of what opening `name` relative to `folder` opens, as itself and never
    /// what a link names, with the least access that reads it; `None` when nothing is there.
    /// On NTFS the open ignores case as NTFS does, so this is what git and Claude would open.
    fn probe(&self, folder: &Self::Folder, name: &OsStr) -> io::Result<Option<FileId>>;

    /// The file ID of `path` itself, never what a link at it names; `None` when nothing is
    /// there.
    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>>;

    /// The SID of the owner of `path` itself, as text.
    fn owner(&self, path: &Path) -> io::Result<String>;

    /// Moves the item at `from` (itself, not what a link names; its final path must be `from`)
    /// into the folder `parts` below `sandbox`, each part made or opened in turn without
    /// following a link, as `name`. Never replaces: a taken name is
    /// [`SetAsideFailure::NameTaken`], a missing item [`SetAsideFailure::Gone`].
    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
    ) -> Result<(), SetAsideFailure>;

    /// Every pool user's SID as text, from the machine's record (`sandbox_pool_sids` on
    /// Windows); empty when no pool is recorded.
    fn pool(&self) -> io::Result<Vec<String>>;

    /// Hands `path` and every item below it that one of `pool` owns to the desktop user, as
    /// forget hands back a pool user's items, never following a link (S15 `PR4a2`): an item
    /// set aside or restored is the lawyer's throughout, whoever owns its top, so no pool user
    /// keeps an owner's `WRITE_DAC` in it once it serves another matter. What anyone else owns
    /// (a colleague, `Administrators`) keeps its owner and DACL, a hard-linked file included.
    fn own(&self, path: &Path, pool: &[String]) -> io::Result<()>;

    /// Moves `name` in the folder `parts` below `sandbox` (opened as for
    /// [`Volume::move_into`], never made) back to `to`, whose folder's final path must be
    /// `to`'s parent. Never replaces.
    fn move_back(
        &self,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
        to: &Path,
    ) -> Result<(), SetAsideFailure>;

    /// Removes the folder `parts` below `sandbox` when it is empty, then each folder above it
    /// while that is empty, never one of the first `kept` parts (`set-aside` and the matter's
    /// folder): a restore's emptied pass folder and the `within` folders in it. Each opened as
    /// for [`Volume::move_back`], refused if a link, and deleted only on its own handle, which
    /// fails for a folder holding anything; that, or a folder gone, ends it. The restore counts
    /// in `kept` every folder that is itself an item still listed ([`kept_levels`]).
    fn remove_emptied(
        &self,
        sandbox: &Path,
        parts: &[OsString],
        kept: usize,
    ) -> Result<(), SetAsideFailure>;
}

/// Why a pass failed, and the item it failed on, when it was one (N11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Refused {
    pub(super) failure: SetAsideFailure,
    /// The item that couldn't be read or moved.
    pub(super) at: Option<PathBuf>,
}

impl Refused {
    /// `failure` on the item `at`.
    pub(super) fn at(failure: SetAsideFailure, at: &Path) -> Refused {
        Refused {
            failure,
            at: Some(at.to_owned()),
        }
    }

    /// As the crate's error.
    #[cfg(windows)]
    pub(super) fn into_error(self) -> crate::Error {
        crate::Error::SetAside {
            failure: self.failure,
            at: self.at,
        }
    }
}

impl From<SetAsideFailure> for Refused {
    fn from(failure: SetAsideFailure) -> Refused {
        Refused { failure, at: None }
    }
}

/// See [`super::before_launch`]. The scan runs before the matter's lock is taken, so a long
/// scan never holds up another session's pass; anything that pass moves meanwhile is gone
/// here, and passed over.
pub(super) fn before_launch(
    volume: &impl Volume,
    root: &Path,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Checked, Refused> {
    let found = scan(volume, root);
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    // A session is about to start: the last clean close no longer describes the matter.
    super::fast::spend_clean_close(data_dir, matter)?;
    let mut moved = Vec::new();
    let mut aside: Vec<Aside> = Vec::new();
    let mut sessions = BTreeSet::new();
    if let Some(record) = place.read_record()? {
        if !record.sessions.is_empty() {
            tracing::info!(
                open = record.sessions.len(),
                "a session of this matter is open; checking its protected names"
            );
            aside = check(volume, &place, &found, &record, now, &mut moved)?;
        }
        let (still, crashed): (BTreeSet<String>, BTreeSet<String>) = record
            .sessions
            .into_iter()
            .partition(|open| running.contains(&open.as_str()));
        if !crashed.is_empty() {
            tracing::info!(
                crashed = crashed.len(),
                "sessions no longer running were checked and are closed"
            );
        }
        sessions = still;
    }
    sessions.insert(session.to_owned());
    let seen = snapshot(&found, &aside)?;
    place.write_record(&Record::new(*matter, sessions, seen))?;
    Ok(Checked {
        moved,
        unscanned: found.unscanned,
    })
}

/// See [`super::after_session`]. No record at all is [`SetAsideFailure::Record`]: there is
/// nothing to tell a change by. The scan runs before the lock is taken, as at launch.
pub(super) fn after_session(
    volume: &impl Volume,
    root: &Path,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Checked, Refused> {
    let found = scan(volume, root);
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    let record = place.read_record()?.ok_or(SetAsideFailure::Record)?;
    let mut moved = Vec::new();
    let aside = check(volume, &place, &found, &record, now, &mut moved)?;
    let mut sessions = record.sessions;
    if found.unscanned.is_empty() {
        sessions.remove(session);
    } else {
        tracing::warn!("the scan was partial; the session stays open, so the next launch checks");
    }
    let seen = snapshot(&found, &aside)?;
    place.write_record(&Record::new(*matter, sessions, seen))?;
    Ok(Checked {
        moved,
        unscanned: found.unscanned,
    })
}

/// See [`super::set_aside_path`]: the item under its own name in the pass's folder.
pub(super) fn set_aside_path(
    volume: &impl Volume,
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock().map_err(|failure| Refused::at(failure, path))?;
    set_aside(volume, &place, path, &[], reason, now)
}

/// See [`super::list_set_aside`].
pub(super) fn list(
    volume: &impl Volume,
    data_dir: &Path,
    matter: &MatterId,
) -> Result<Vec<Entry>, Refused> {
    let place = Place::new(data_dir, matter);
    let _held = place.lock()?;
    Ok(load(volume, &place)?.entries)
}

/// See [`super::restore_set_aside`]. The manifest's own copy of the entry is used, found by its
/// number, and only when `entry` names the same item; its folder, the folders within it and
/// its name must each be one plain path part.
pub(super) fn restore(volume: &impl Volume, data_dir: &Path, entry: &Entry) -> Result<(), Refused> {
    let place = Place::new(data_dir, &entry.matter);
    let _held = place.lock()?;
    let mut manifest = load(volume, &place)?;
    let at = manifest
        .entries
        .iter()
        .position(|e| e.id == entry.id && e.from == entry.from && e.to == entry.to)
        .ok_or(SetAsideFailure::Gone)?;
    let found = manifest.entries.remove(at);
    let plain = one_part(OsStr::new(&found.folder))
        && one_part(&found.name)
        && found.within.iter().all(|part| one_part(part));
    if !plain || !found.from.is_absolute() {
        return Err(SetAsideFailure::Record.into());
    }
    if holds_git(&found) {
        tracing::info!("a set-aside .git, or an item inside one, is never put back");
        return Err(SetAsideFailure::GitFolder.into());
    }
    volume.move_back(
        &place.sandbox,
        &place.parts(&found.folder, &found.within),
        &found.name,
        &found.from,
    )?;
    tracing::info!(reason = ?found.reason, "a set-aside item was restored");
    place.write_manifest(&manifest)?;
    // Only now, with the manifest listing the item no more, the folders it leaves empty go, so a
    // crash never leaves a manifest naming a folder that is gone. One left (not empty, or not
    // removed) only means the uninstaller keeps the set-aside; it never fails the restore.
    // Never a folder that is itself an item still listed (a move reuses a folder already there):
    // the removal stops below the deepest such folder on the way.
    let parts = place.parts(&found.folder, &found.within);
    let listed: Vec<Vec<OsString>> = manifest
        .entries
        .iter()
        .chain(&manifest.intents)
        .map(|item| {
            let mut path = place.parts(&item.folder, &item.within);
            path.push(item.name.clone());
            path
        })
        .collect();
    let kept = kept_levels(&parts, &listed, 2);
    if let Err(failure) = volume.remove_emptied(&place.sandbox, &parts, kept) {
        tracing::warn!(%failure, "a restore's emptied set-aside folder stays");
    }
    // The item is back and listed no more, so a failure here doesn't undo the restore; the
    // next launch's apply hands a pool user's protected item back before its deny anyway.
    hand_over(volume, &found.from, "a restored item");
    Ok(())
}

/// How many leading folders of `parts` a restore's removal must keep: at least `kept`, and every
/// folder down to the deepest one that is the path of an item in `listed` (parts compared
/// ignoring ASCII case, as NTFS matches names).
pub(super) fn kept_levels(parts: &[OsString], listed: &[Vec<OsString>], kept: usize) -> usize {
    let same = |a: &[OsString], b: &[OsString]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
    };
    (kept + 1..=parts.len())
        .rev()
        .find(|end| {
            let folder = parts.get(..*end).unwrap_or_default();
            listed.iter().any(|item| same(folder, item))
        })
        .unwrap_or(kept)
}

/// Whether `entry`'s item is a `.git` or sits inside one: its own name, a folder it was in below
/// the matter, or any part of the path it would go back to opens as `.git` ([`is_git`]). The
/// restore's folder is opened by that path and must have it as its final path (`move_back`), so
/// no short name or link can land it inside a `.git` the words don't name.
pub(super) fn holds_git(entry: &Entry) -> bool {
    let in_path = entry.from.components().any(|part| match part {
        Component::Normal(name) => is_git(name),
        Component::Prefix(_) | Component::RootDir | Component::CurDir | Component::ParentDir => {
            false
        }
    });
    in_path || is_git(&entry.name) || entry.within.iter().any(|part| is_git(part))
}

/// Whether `name` is one plain path part: no separator, drive, `.` or `..`. Text or not.
pub(super) fn one_part(name: &OsStr) -> bool {
    let mut parts = Path::new(name).components();
    matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !name
        .as_encoded_bytes()
        .iter()
        .any(|byte| matches!(byte, b'/' | b'\\' | b':'))
}
