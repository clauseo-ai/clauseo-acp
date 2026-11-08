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
use super::scan::{Kind, Listed, Scan, scan};
use super::{Checked, Entry, Reason, SetAsideFailure};

mod refusals;

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

    /// At most `max` bytes of the file `name` in `folder`, opened relative to it as itself (a
    /// link there is an error), never waiting on another process: on NTFS with
    /// `FILE_COMPLETE_IF_OPLOCKED`, and an open that finds an oplock's break under way is an
    /// error, as unreadable. For a `commondir` an agent may have made and holds an oplock on.
    fn read_small(&self, folder: &Self::Folder, name: &OsStr, max: u64) -> io::Result<Vec<u8>>;

    /// Opens the top of a volume (a drive's root, `C:\`) as itself, to walk a path down from it
    /// one part at a time. Only the top the watched folder's own path starts with is opened,
    /// never a network path (`scan/locate.rs`).
    fn open_top(&self, top: &Path) -> io::Result<Self::Folder>;

    /// What opening `name` relative to `folder` opens, as itself and never what a link names,
    /// as [`Volume::probe`] opens it: its file ID and what it is; `None` when nothing is there.
    fn look(&self, folder: &Self::Folder, name: &OsStr) -> io::Result<Option<(FileId, Kind)>>;

    /// The target the link `name` in `folder` names, as the link spells it (relative or not),
    /// read from the link's own handle, opened relative to `folder` as itself and never followed
    /// (`FSCTL_GET_REPARSE_POINT` on NTFS). Anything but a junction or symbolic link is an error.
    fn link_target(&self, folder: &Self::Folder, name: &OsStr) -> io::Result<PathBuf>;

    /// The SID of the owner of `name` in `folder`, opened relative to it as itself, as text.
    fn owner_at(&self, folder: &Self::Folder, name: &OsStr) -> io::Result<String>;

    /// The name `folder` has in its parent as the volume keeps it (on NTFS its long name, even
    /// when it was opened by its short one), read from the open folder.
    fn folder_name(&self, folder: &Self::Folder) -> io::Result<OsString>;

    /// The file ID of `path` itself, never what a link at it names; `None` when nothing is
    /// there. A read by path: the live watch reads through handles instead (`live.rs`).
    fn file_id(&self, path: &Path) -> io::Result<Option<FileId>>;

    /// The SID of the owner of `path` itself, as text. A read by path, as [`Volume::file_id`].
    fn owner(&self, path: &Path) -> io::Result<String>;

    /// The folder where what is set aside out of `folder` (the folder an item is in, or the
    /// matter's own) starts its chain: `sandbox` itself when `folder` is on its volume, else the
    /// lawyer's `<folder's volume root>\.clauseo-set-aside-<SID>` ([`super::drive_folder_name`] of
    /// this thread's user), since a rename can't cross volumes. Volumes are told apart by their
    /// roots (`GetVolumePathNameW` on NTFS), which need no handle, so a missing item's folder has
    /// one too, and a volume mounted on a folder is its own.
    fn items_root(&self, sandbox: &Path, folder: &Path) -> io::Result<PathBuf>;

    /// Makes, or checks when there, the folder `root` an [`Volume::items_root`] gave other than
    /// the sandbox folder, then each of `parts` below it in turn (`set-aside` and the matter's
    /// folder), as [`Place::lock`] does the sandbox folder's: refused if a link or not a folder,
    /// secured for the lawyer alone (`private_dir::make_drive_folder` on NTFS). A `root` already
    /// there that another account owns is refused with [`not_yours`]'s error, never taken back.
    fn secure_items_root(&self, root: &Path, parts: &[OsString]) -> io::Result<()>;

    /// Moves the item at `from` (itself, not what a link names; its final path must be `from`)
    /// into the folder `parts` below `sandbox`, each part made or opened in turn without
    /// following a link, as `name`. Never replaces: a taken name is
    /// [`SetAsideFailure::NameTaken`], a missing item [`SetAsideFailure::Gone`]. The owner is
    /// read from the very handle that is renamed, and returned (`None` when it can't be read);
    /// one `need` refuses isn't moved ([`SetAsideFailure::NotTheAgents`]).
    fn move_into(
        &self,
        from: &Path,
        sandbox: &Path,
        parts: &[OsString],
        name: &OsStr,
        need: Need<'_>,
    ) -> Result<Option<String>, SetAsideFailure>;

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

/// Whose an item must be for [`Volume::move_into`] to move it, read from the item's own handle
/// once it is opened as itself, before the rename.
#[derive(Debug, Clone, Copy)]
pub(super) enum Need<'a> {
    /// Anyone's.
    Anyone,
    /// One of these SIDs' (the pool's, exactly): a folder the live watch sets aside whole
    /// unchecked, which must be the agent's (`live.rs`). An owner that can't be read counts as
    /// the agent's and moves: the agent can deny the lawyer read access on what it made, and
    /// that must not keep a folder too big to check in the matter.
    Pool(&'a [String]),
}

impl Need<'_> {
    /// Whether an item whose owner reads as `owner` may move.
    pub(super) fn allows(self, owner: Option<&str>) -> bool {
        match self {
            Need::Anyone => true,
            Need::Pool(pool) => owner.is_none_or(|owner| pool.iter().any(|sid| sid == owner)),
        }
    }
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

/// What [`Volume::secure_items_root`] fails with when the drive folder there is another
/// account's: [`SetAsideFailure::DriveFolderNotYours`], not [`SetAsideFailure::DriveFolder`].
#[derive(Debug)]
struct NotYours;

impl std::fmt::Display for NotYours {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("the set-aside folder on the drive is another account's")
    }
}

impl std::error::Error for NotYours {}

/// The error for a drive folder another account owns ([`NotYours`]).
pub(crate) fn not_yours() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, NotYours)
}

/// Whether `error` is [`not_yours`]'s.
fn is_not_yours(error: &io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(<dyn std::error::Error + Send + Sync>::is::<NotYours>)
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
    let place = Place::new(data_dir, matter);
    launch_pass(volume, (root, data_dir), &place, session, running, now)
}

/// [`before_launch`] on the matter's notes folder `notes`, with its own record
/// ([`Place::notes`]); what it moves goes to the matter's set-aside. A notes folder not there
/// yet is recorded as empty, so whatever the session makes in it is new after.
pub(super) fn notes_before_launch(
    volume: &impl Volume,
    notes: &Path,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Checked, Refused> {
    let place = Place::notes(data_dir, matter);
    launch_pass(volume, (notes, data_dir), &place, session, running, now)
}

/// [`after_session`] on the matter's notes folder `notes`, against its own record.
pub(super) fn notes_after_session(
    volume: &impl Volume,
    notes: &Path,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Checked, Refused> {
    let place = Place::notes(data_dir, matter);
    session_pass(volume, notes, &place, session, now)
}

/// What `a` and then `b` moved, and the folders either couldn't scan.
#[cfg(windows)]
pub(super) fn both(mut a: Checked, b: Checked) -> Checked {
    a.moved.extend(b.moved);
    a.unscanned.extend(b.unscanned);
    a
}

/// The scan of `root` for `place`'s pass: a notes folder that isn't there is an empty scan,
/// not a partial one (it is made when first written).
fn scan_for(volume: &impl Volume, root: &Path, place: &Place) -> Scan {
    if place.is_notes() && matches!(volume.file_id(root), Ok(None)) {
        return Scan::default();
    }
    scan(volume, root)
}

/// The launch pass over `root` with `place`'s record, as [`before_launch`] says; `data_dir`
/// holds the matter's clean-close record, which a launch pass on the matter spends.
fn launch_pass(
    volume: &impl Volume,
    (root, data_dir): (&Path, &Path),
    place: &Place,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Checked, Refused> {
    let found = scan_for(volume, root, place);
    let _held = place.lock()?;
    items_root(volume, place, root)?;
    if !place.is_notes() {
        // A session is about to start: the last clean close no longer describes the matter.
        super::fast::spend_clean_close(data_dir, &place.matter())?;
    }
    let mut moved = Vec::new();
    let mut aside: Vec<Aside> = Vec::new();
    let mut sessions = BTreeSet::new();
    if let Some(record) = place.read_record()? {
        if !record.sessions.is_empty() {
            tracing::info!(
                open = record.sessions.len(),
                "a session of this matter is open; checking its protected names"
            );
            aside = check(volume, place, &found, &record, now, &mut moved)?;
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
    place.write_record(&Record::new(place.matter(), sessions, seen))?;
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
    let place = Place::new(data_dir, matter);
    session_pass(volume, root, &place, session, now)
}

/// The after-session pass over `root` with `place`'s record, as [`after_session`] says.
fn session_pass(
    volume: &impl Volume,
    root: &Path,
    place: &Place,
    session: &str,
    now: SystemTime,
) -> Result<Checked, Refused> {
    let found = scan_for(volume, root, place);
    let _held = place.lock()?;
    let record = place.read_record()?.ok_or(SetAsideFailure::Record)?;
    let mut moved = Vec::new();
    let aside = check(volume, place, &found, &record, now, &mut moved)?;
    let mut sessions = record.sessions;
    if found.unscanned.is_empty() {
        sessions.remove(session);
    } else {
        tracing::warn!("the scan was partial; the session stays open, so the next launch checks");
    }
    let seen = snapshot(&found, &aside)?;
    place.write_record(&Record::new(place.matter(), sessions, seen))?;
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
    set_aside(volume, &place, path, &[], reason, now, Need::Anyone)
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
/// its name must each be one plain path part. Besides a git folder, it refuses what was git's
/// in a folder a launch `commondir` names, and a link leading out (`refusals.rs`).
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
    let common = refusals::common_targets(data_dir, &entry.matter);
    let git = refusals::holds_git(&found) || refusals::holds_repo(volume, &found);
    if git || refusals::from_common(&found, &common) {
        tracing::info!("a set-aside git folder, or an item inside one, is never put back");
        return Err(SetAsideFailure::GitFolder.into());
    }
    if refusals::leads_out(volume, &found) {
        tracing::info!("a set-aside link leading out of the matter is never put back");
        return Err(SetAsideFailure::LinkOut.into());
    }
    let parts = place.parts(&found.folder, &found.within);
    let root = root_of(volume, &place, &found, &parts)?;
    volume.move_back(&root, &parts, &found.name, &found.from)?;
    tracing::info!(reason = ?found.reason, "a set-aside item was restored");
    place.write_manifest(&manifest)?;
    // Only now, with the manifest listing the item no more, the folders it leaves empty go, so a
    // crash never leaves a manifest naming a folder that is gone. One left (not empty, or not
    // removed) only means the uninstaller keeps the set-aside; it never fails the restore.
    // Never a folder that is itself an item still listed (a move reuses a folder already there):
    // the removal stops below the deepest such folder on the way.
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
    if let Err(failure) = volume.remove_emptied(&root, &parts, kept) {
        tracing::warn!(%failure, "a restore's emptied set-aside folder stays");
    }
    // The item is back and listed no more, so a failure here doesn't undo the restore; the
    // next launch's apply hands a pool user's protected item back before its deny anyway.
    hand_over(volume, &found.from, "a restored item");
    Ok(())
}

/// The folder `place`'s matter's set-aside starts from for what is set aside out of `folder`
/// ([`Volume::items_root`]), made or checked first when it isn't the sandbox folder
/// ([`Volume::secure_items_root`]). Every launch checks the matter's own folder's, and every
/// move its item's folder's, so a folder that can't be made or secured refuses the launch, and
/// no item is left in place for want of one. A failure is [`SetAsideFailure::DriveFolder`] at
/// that folder (at `folder` when its volume's root can't be read), or
/// [`SetAsideFailure::DriveFolderNotYours`] at it when another account owns it.
pub(super) fn items_root(
    volume: &impl Volume,
    place: &Place,
    folder: &Path,
) -> Result<PathBuf, Refused> {
    let refuse = |at: &Path, error: &io::Error| {
        if is_not_yours(error) {
            tracing::warn!("the set-aside folder on the matter's drive is another account's");
            return Refused::at(SetAsideFailure::DriveFolderNotYours, at);
        }
        let kind = error.kind();
        tracing::warn!(%kind, "the set-aside folder on the matter's drive can't be made");
        tracing::debug!(%error, "the set-aside folder on the matter's drive");
        Refused::at(SetAsideFailure::DriveFolder, at)
    };
    let root = volume
        .items_root(&place.sandbox, folder)
        .map_err(|error| refuse(folder, &error))?;
    if root != place.sandbox {
        volume
            .secure_items_root(&root, &place.matter_parts())
            .map_err(|error| refuse(&root, &error))?;
    }
    Ok(root)
}

/// The folder `found`'s chain (`parts`, then its name) starts from, read off its `to`: each
/// last part of `to` must be the one `parts` and its name say. It is trusted only when it is the
/// sandbox folder or the one [`Volume::items_root`] gives for the folder the item goes back to:
/// this lawyer's own drive folder there, named for its SID ([`super::drive_folder_name`]), never
/// another lawyer's, a set-aside squatter's or the unnamed `.clauseo-set-aside`. Anything else is
/// [`SetAsideFailure::Record`], so a manifest never sends a restore elsewhere.
fn root_of(
    volume: &impl Volume,
    place: &Place,
    found: &Entry,
    parts: &[OsString],
) -> Result<PathBuf, Refused> {
    let mut root = found.to.as_path();
    for part in parts.iter().chain([&found.name]).rev() {
        if root.file_name() != Some(part.as_os_str()) {
            return Err(SetAsideFailure::Record.into());
        }
        root = root.parent().ok_or(SetAsideFailure::Record)?;
    }
    if root == place.sandbox {
        return Ok(root.to_owned());
    }
    let folder = found.from.parent().ok_or(SetAsideFailure::Record)?;
    match volume.items_root(&place.sandbox, folder) {
        Ok(expected) if expected == root => Ok(expected),
        Ok(_) | Err(_) => {
            tracing::warn!("a set-aside entry names a folder that isn't its drive's; refused");
            Err(SetAsideFailure::Record.into())
        }
    }
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
