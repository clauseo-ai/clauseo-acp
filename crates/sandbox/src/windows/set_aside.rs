//! Protected names and links in a matter that changed in a session, set aside where the agent
//! can't reach them (S15 PR3; the reviews' H2, M6, N1, N6, N10, N11 and round 3's R1 to R4).
//! NTFS holds no entry for a missing name, so a protected name
//! ([`PROTECTED_NAMES`](crate::PROTECTED_NAMES), and `.git`) that doesn't exist at launch gets
//! no deny, and the agent can make it; it can also rename or hard-link a lawyer's own file or
//! folder to one, which keeps the lawyer as the owner, so git's ownership check passes and its
//! hooks run as the lawyer. So the check goes by change, not by owner. It covers each protected
//! name at the matter's root, every `.git`, `.claude`, `.mcp.json`, `CLAUDE.md` and `AGENTS.md`
//! at any depth (`scan::NESTED_NAMES`, case ignored on NTFS), and every link (junction or
//! symbolic link) anywhere in the matter tree, whatever its name: a link the agent makes could
//! lead git to a `.git` outside the matter (R2).
//!
//! Each pass finds them with its own scan of the matter tree (`scan.rs`): read-only, with no
//! cap, never following a link, each protected name probed by opening it relative to its
//! folder's handle, so whatever NTFS opens for `.git` counts. It doesn't use the ACL walk's
//! results, so a walk that refuses never skips the check (R1).
//!
//! - At launch (`before_launch`), each protected name at the root is recorded with its file ID
//!   (the volume's serial and the 128-bit id, read from a handle), or as absent, and so is each
//!   nested name and each link the scan found; the session is added to the record's open
//!   sessions.
//! - When the agent exits (`after_session`), every protected name or link that is new, or whose
//!   file ID changed, is moved aside, whoever owns it (a link as itself, never what it names);
//!   then everything is recorded afresh and the session is taken out. A name or link the record
//!   has and the scan no longer finds is dropped.
//! - While any session is open, running or ended without an after-session pass, each launch
//!   runs the same check before recording afresh. Two sessions of one matter may run at once:
//!   the record is clean only when no session is open, and a protected name a running
//!   session's agent made may be set aside when another session starts. A launch is told which
//!   of the matter's sessions still run; every other open session crashed. Its changes are
//!   checked at that launch, then its id is dropped (R3). PR5 and PR6 call `before_launch`
//!   only once a crashed session's job object is closed (kill-on-close), so its agent is dead.
//! - A scan that can't open or list a folder still checks and sets aside what it found, and
//!   the pass names the folder ([`Checked::unscanned`]) for the running page. After the
//!   session, it keeps the session open, so the next launch checks again.
//! - The strict walk moves one item the lawyer can't open the same way (`set_aside_path`).
//!
//! The order a launch follows (PR6): `before_launch`, then `apply`; when the agent exits,
//! `repair_after_session`, then `after_session`. The set-aside and the ACL calls are
//! independent: neither takes the other's result, and a refused `apply` or repair doesn't stop
//! the set-aside's check. `before_launch` comes first, so a crashed session is checked even
//! when `apply` then refuses.
//!
//! A protected name the lawyer changes during a session is set aside too, which is accepted.
//!
//! The matter's notes folder (`Policy::notes`), which the agent can write too, gets the same
//! passes at launch and after the session, with its own record (`notes-record.json`, its keys
//! below the notes folder) beside the matter's, under the matter's lock; what they move goes to
//! the matter's set-aside and manifest. Its live watch starts from that record, as the matter's
//! does, so what is in notes at launch counts as known only once a launch pass checked it. The
//! launch check's fast path covers the matter alone: the notes folder always gets its full pass.
//!
//! Everything lives in `<data_dir>\sandbox\set-aside\<matter>\`, under the sandbox folder's
//! protected DACL, never in the state folder, which the agent can write: `record.json` (the
//! names and links at the last pass, and the open sessions), `manifest.json` (each item set
//! aside: [`Entry`]), `set-aside.lock`, which every read and write of the record or the
//! manifest holds, and under which the temp files a crash left beside either are removed
//! (`files.rs`), and one folder per pass named for its UTC time (`YYYYMMDDTHHMMSS-mmmZ`)
//! holding the items, each at its own path below the matter's root (`<pass>\sub\.git`), so
//! nested names never clash; a real clash gets `~2` up to `~9`. Each move is written to the
//! manifest as an intent first and finished after, and each load of the manifest settles an
//! intent a crash left: the item at its new place finishes the entry, the item still at its old
//! place drops it. On Windows each part of that folder is opened relative to its parent's
//! handle, never through a link, and each item is moved by a rename on its own handle
//! (`ntfs.rs`). `list_set_aside` and `restore_set_aside` show and undo a set-aside; a restore
//! never overwrites, and once the manifest lists the item no more, the folders it leaves empty
//! (its pass folder, the folders within it) go, by handle and only if empty
//! (`Volume::remove_emptied`), so a matter whose items were all restored holds only bookkeeping.
//!
//! A rename can't cross volumes, so an item whose folder is on another volume than the data
//! folder goes to that volume's own set-aside folder instead, one per lawyer
//! (`<volume root>\.clauseo-set-aside-<lawyer's SID>`, [`drive_folder_name`]), holding
//! `set-aside\<matter>\<pass>\...` as the sandbox folder does (`Volume::items_root`). It is made
//! with its own protected DACL (the lawyer and `SYSTEM`, and a deny for the pool group) and
//! secured by the sandbox folder's mechanics (`private_dir::make_drive_folder`), except that one
//! already there the lawyer doesn't own is never taken back: a launch that finds one is refused
//! ([`SetAsideFailure::DriveFolderNotYours`]) until `SYSTEM` sets it aside by its handle,
//! contents unopened (`heal.rs`, from the refresh task). Another lawyer's folder on the same
//! volume has another name, so it never refuses this lawyer. It is made at each launch for the
//! matter's own folder and checked again before each move into it; a launch that can't make it
//! (a read-only drive, one that keeps no permissions, a link there) is refused
//! ([`SetAsideFailure::DriveFolder`]). The record, the manifest and the lock stay in the data
//! folder. The uninstaller and forget (`bookkeeping.rs`, `ntfs/forget.rs`) know only the data
//! folder's set-aside, not these.
//!
//! A set-aside that can't happen refuses the launch with [`crate::Error::SetAside`], whose
//! `at` names the item for the running page: an item a rename still can't move across volumes
//! (a volume mounted on a folder inside the matter: [`SetAsideFailure::OtherVolume`], nothing
//! is copied), or one whose folder doesn't give the lawyer `FILE_DELETE_CHILD`
//! (`Io(PermissionDenied)`). The name must then be moved or removed by hand.
//!
//! Known limits:
//! - The check compares file IDs only, so a nested protected item is caught when it is
//!   created, replaced, renamed in or swapped (a new file ID), not when it is edited in place.
//!   The agent can't edit one in place: every nested name that exists at launch is
//!   write-denied to the pool user, as the root names are (`Policy::read_only_in_matter`,
//!   S15 `PR4a2`), so an in-place edit is the lawyer's own and stays.
//! - A nested protected item one session's agent makes is visible to a second session of the
//!   matter running at the same time until a set-aside pass runs: a permission entry can't deny
//!   a name that doesn't exist yet (accepted).
//! - A `CLAUDE.local.md` or `AGENTS.override.md` below the root is not checked (D-B17: two
//!   more opens per folder cost over 5% of the scan).
//!
//! A lawyer's own link that was
//! there at launch and is unchanged stays; a link made or swapped while a session ran is set
//! aside even when the lawyer made it. An item moved aside keeps its own DACL, so an agent it
//! grants could still open it by its full path; it no longer loads in the matter. One another
//! user owns (the agent's) is then handed to the desktop user and worked out again from the
//! set-aside folder (S15 `PR4a2`, `moves.rs`); its entry keeps the owner it had.

use std::ffi::OsString;
use std::io;
#[cfg(windows)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(windows)]
use std::time::SystemTime;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};

/// The set-aside's folder in the sandbox folder (`<data_dir>\sandbox\set-aside\<matter>`). Public
/// so the uninstaller keeps it by this name, not by a copy of the string.
pub const SET_ASIDE_DIR: &str = "set-aside";
/// The set-aside's own bookkeeping in each matter's folder (`files.rs`): the record and the lock,
/// which every launch leaves, the notes folder's record, which a launch with a notes folder
/// leaves, and the manifest, which a pass that set something aside leaves and
/// a restore of every item empties. Public so the uninstaller counts a matter's folder holding
/// only these as empty; `--remove` deletes them once the matter is forgotten (`bookkeeping.rs`),
/// each only when its contents allow (a closed record, a manifest with nothing outstanding).
pub const SET_ASIDE_BOOKKEEPING: [&str; 4] = [
    "record.json",
    "set-aside.lock",
    "manifest.json",
    "notes-record.json",
];
/// The start of the name of a lawyer's set-aside folder at the root of a volume other than the
/// data folder's ([`drive_folder_name`]): what is set aside from a matter on that volume goes
/// there, since a rename can't cross volumes.
pub const ITEMS_ROOT: &str = ".clauseo-set-aside";
/// What follows a drive folder's own name once `SYSTEM` set it aside because another account
/// made it (`heal.rs`): `<name>.squatted-<UTC second>-<8 random hex digits>`.
#[cfg(any(windows, test))]
pub(crate) const SQUATTED: &str = ".squatted-";

/// The name of the set-aside folder the lawyer whose SID is `user` (its string form) has at the
/// root of a volume other than the data folder's: `.clauseo-set-aside-<user>` ([`ITEMS_ROOT`]).
#[cfg(any(windows, test))]
pub(crate) fn drive_folder_name(user: &str) -> String {
    format!("{ITEMS_ROOT}-{user}")
}

#[cfg(any(windows, test))]
pub(crate) use bookkeeping::forget_bookkeeping;
#[cfg(any(windows, test, feature = "test-support"))]
pub use bookkeeping::{SetAsideBookkeeping, forget_set_aside_bookkeeping};
#[cfg(test)]
pub(crate) use fast::open_session;
#[cfg(any(windows, test))]
pub(crate) use fast::{open_sessions_of, under_lock};

#[cfg(any(windows, test, feature = "test-support"))]
mod bookkeeping;
#[cfg(any(windows, test))]
mod check;
#[cfg(any(windows, test))]
mod fast;
#[cfg(any(windows, test, feature = "test-support"))]
mod files;
#[cfg(any(windows, test))]
pub(crate) mod heal;
#[cfg(any(windows, test))]
mod live;
#[cfg(test)]
pub(in crate::windows) use live::Moved;
#[cfg(any(windows, test))]
pub(in crate::windows) use live::{Ending, Failed, Hooks, Live, SCAN_BUDGET};
#[cfg(any(windows, test))]
mod moves;
mod names;
#[cfg(windows)]
mod ntfs;
#[cfg(any(windows, test))]
mod repo_names;
#[cfg(windows)]
pub(in crate::windows) use ntfs::make_folders;
#[cfg(any(windows, test))]
mod run;
#[cfg(any(windows, test))]
pub(crate) use run::not_yours;
#[cfg(any(windows, test))]
mod scan;
#[cfg(any(windows, test))]
mod time;

/// Why an item was set aside.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    /// The name or link wasn't there at the last pass.
    New,
    /// The name or link was there at the last pass as another file, folder or link: its file
    /// ID changed.
    Changed,
    /// The lawyer can't open it: its permissions shut the lawyer out.
    LawyerCantOpen,
}

/// One item set aside, as the manifest lists it. Its paths and names are kept as the file
/// system gave them, text or not (a lone surrogate, say): the manifest writes one that isn't
/// text as its UTF-16 code units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Its number in the matter's manifest, never reused.
    pub id: u64,
    /// The matter it was in.
    pub matter: MatterId,
    /// Where it was, in the matter.
    #[serde(with = "names::path")]
    pub from: PathBuf,
    /// Where it is now: `<data_dir>\sandbox\set-aside\<matter>\<folder>\<within...>\<name>`, or
    /// the same below the lawyer's set-aside folder on the item's own volume
    /// ([`drive_folder_name`]) when the matter is on another volume than the data folder.
    #[serde(with = "names::path")]
    pub to: PathBuf,
    /// The pass's folder, named for its UTC time.
    pub folder: String,
    /// The folders below the pass's folder it is in: its own folders below the matter's root
    /// (`sub` for `sub\.git`). Empty for a name at the root and for the walk's item.
    #[serde(with = "names::many")]
    pub within: Vec<OsString>,
    /// Its name in that folder: its own name, with `~2` and so on added if that was taken.
    #[serde(with = "names::one")]
    pub name: OsString,
    /// Why it was moved.
    pub reason: Reason,
    /// Its owner's SID when it was moved, for information; none when it couldn't be read.
    pub owner: Option<String>,
    /// When it was moved, in UTC (RFC 3339, to the millisecond).
    pub at: String,
}

/// What a pass did: the items it set aside, and the folders its scan couldn't open or list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    /// What moved, for the lawyer's notice.
    pub moved: Vec<Entry>,
    /// The folders in the matter the scan couldn't open or list (the matter's own folder, when
    /// it couldn't open that). Nothing inside them was checked: the running page tells the
    /// lawyer, naming them. Empty when the scan was complete.
    pub unscanned: Vec<PathBuf>,
}

impl Checked {
    /// Whether the scan was partial: some folder couldn't be opened or listed.
    pub fn partial(&self) -> bool {
        !self.unscanned.is_empty()
    }
}

/// Why a set-aside, its record or a restore failed. Names no path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SetAsideFailure {
    /// Something is at the name the item would move to; nothing was overwritten.
    #[error("the name is taken")]
    NameTaken,
    /// The item isn't there any more, or the manifest no longer lists it.
    #[error("the item is gone")]
    Gone,
    /// The item and the folder it would move to are on different volumes, and an item can't be
    /// renamed across volumes (`ERROR_NOT_SAME_DEVICE`); nothing is copied. A matter on another
    /// drive than the data folder has its own set-aside folder there, so this is a volume
    /// mounted on a folder inside the matter, or one the volume roots don't tell apart.
    #[error("the matter is on another drive than Clauseo's data folder")]
    OtherVolume,
    /// A folder on the way is a link (a reparse point), or the item's final path isn't the one
    /// planned.
    #[error("a folder on the way is a link, or the item moved")]
    Changed,
    /// The record or the manifest is missing where one is needed, or can't be read or trusted.
    #[error("the set-aside record is missing or can't be read")]
    Record,
    /// A file-system call failed, of this kind.
    #[error("a file-system call failed ({0})")]
    Io(io::ErrorKind),
    /// A restore of a `.git`, of an item inside one, or of anything holding a `HEAD` or a
    /// `commondir` (a bare repository's or a worktree's git folder) is refused: put back, it would
    /// be the lawyer's, and git trusts a lawyer's git folder, so its hooks or `core.fsmonitor`
    /// would run as the lawyer.
    #[error("{}", GIT_REFUSAL)]
    GitFolder,
    /// The lawyer's set-aside folder on the matter's own volume ([`drive_folder_name`]) can't be
    /// made or secured: the volume is read-only or keeps no ACLs, the folder is a link or not a
    /// folder, or the volume's root can't be read.
    #[error(
        "Clauseo can't make a private folder on the matter's drive for what it moves out of \
             the matter"
    )]
    DriveFolder,
    /// The lawyer's set-aside folder on the matter's own volume ([`drive_folder_name`]) is there
    /// and another account owns it (one that made it first, a pool user before the launch). It
    /// is never taken back: `SYSTEM` sets it aside by its handle, unopened, and makes the
    /// lawyer's afresh (`heal.rs`, run by the refresh task, [`crate::windows::refresh`]), and
    /// the next launch goes on.
    #[error(
        "Clauseo's private folder on the matter's drive belongs to another account, so Clauseo \
         won't use it until its system task moves it aside"
    )]
    DriveFolderNotYours,
    /// The live watch moves a folder whole unchecked only when a pool user (the agent) owns it,
    /// read from the handle the move renames (`run::Need::Pool`); this one's owner is someone
    /// else's, or can't be read, so it stays where it is for the check after the session.
    #[error("the item isn't the agent's, so it stays where it is")]
    NotTheAgents,
    /// A restore of a link (a junction or a symbolic link) whose target, read by its words,
    /// leads outside the folder it was set aside from, or can't be read, is refused: put back,
    /// it would lead git, or the lawyer's own tools, outside the matter.
    #[error("{}", LINK_REFUSAL)]
    LinkOut,
}

/// [`SetAsideFailure::LinkOut`]'s words, for the lawyer.
const LINK_REFUSAL: &str = "Clauseo won't put back a link that leads outside the matter: your \
                            own tools would follow it out of the matter";

/// [`SetAsideFailure::GitFolder`]'s words, for the lawyer.
const GIT_REFUSAL: &str = "Clauseo won't put back a git folder or repository an agent made: your \
                           own git tools would run code from it as you";

/// The launch-time call, before `apply`. Scans the matter; when any session of the matter is
/// open (running, or ended with no [`after_session`]), sets aside each protected name and link
/// that is new or changed since the last pass. Then drops each open session not in `running`
/// (the ids of this matter's sessions PR6 still runs): those crashed, and were just checked.
/// PR5 and PR6 call this only once a crashed session's job object is closed (kill-on-close),
/// so its agent is dead. Last, records each name and link afresh and adds `session` to the open
/// sessions. Returns what moved, for the lawyer's notice, and the folders the scan couldn't
/// open or list. On failure the launch is refused and the record stays as it was.
#[cfg(windows)]
pub fn before_launch(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    running: &[&str],
    now: SystemTime,
) -> Result<Checked, crate::Error> {
    let root = policy.matter();
    let checked = run::before_launch(&ntfs::Ntfs, root, data_dir, matter, session, running, now)
        .map_err(run::Refused::into_error)?;
    let Some(notes) = policy.notes() else {
        return Ok(checked);
    };
    run::notes_before_launch(&ntfs::Ntfs, notes, data_dir, matter, session, running, now)
        .map(|more| run::both(checked, more))
        .map_err(run::Refused::into_error)
}

/// [`before_launch`] after the launch check (S15 `PR3d`): when the check found nothing changed in
/// the matter since its last clean close, the last record is kept and only the session added;
/// when it found changes in a few folders, only those are scanned again. In every other case,
/// or when no record is there or a session is open after all, the full pass runs.
#[cfg(windows)]
pub fn before_launch_checked(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    running: &[&str],
    now: SystemTime,
    check: &crate::windows::clean_close::LaunchCheck,
) -> Result<Checked, crate::Error> {
    let root = policy.matter();
    if let Some(touched) = check.touched_in_matter() {
        let fast = fast::before_launch_fast(&ntfs::Ntfs, root, data_dir, matter, session, &touched)
            .map_err(run::Refused::into_error)?;
        if let Some(checked) = fast {
            // The launch check covers the matter alone: the notes folder gets its full pass.
            let Some(notes) = policy.notes() else {
                return Ok(checked);
            };
            return run::notes_before_launch(
                &ntfs::Ntfs,
                notes,
                data_dir,
                matter,
                session,
                running,
                now,
            )
            .map(|more| run::both(checked, more))
            .map_err(run::Refused::into_error);
        }
    }
    before_launch(policy, data_dir, matter, session, running, now)
}

/// The call when the agent has exited, after `repair_after_session` and whatever it returned:
/// scans the matter, sets aside each protected name and link that is new or changed since the
/// last pass, whoever owns it, records them afresh and takes `session` out of the open
/// sessions. When the scan was partial, what it found is still set aside, but the session
/// stays open, so the next launch checks again. Returns what moved and the folders the scan
/// couldn't open or list. On failure the record stays as it was, so the next launch checks
/// again.
#[cfg(windows)]
pub fn after_session(
    policy: &crate::Policy,
    data_dir: &Path,
    matter: &MatterId,
    session: &str,
    now: SystemTime,
) -> Result<Checked, crate::Error> {
    let root = policy.matter();
    let checked = run::after_session(&ntfs::Ntfs, root, data_dir, matter, session, now);
    // The notes folder's pass runs whatever the matter's did.
    let notes = policy
        .notes()
        .map(|notes| run::notes_after_session(&ntfs::Ntfs, notes, data_dir, matter, session, now))
        .transpose();
    match (checked, notes) {
        (Ok(checked), Ok(notes)) => Ok(run::both(checked, notes.unwrap_or_default())),
        (Err(refused), _) | (_, Err(refused)) => Err(refused.into_error()),
    }
}

/// Moves the one item at `path` (the strict walk's child the lawyer can't open: the `DELETE`
/// open succeeds through the parent's `FILE_DELETE_CHILD`) into this pass's set-aside folder,
/// under its own name, with a manifest entry giving `reason`. A failure's `at` is `path`.
/// `matter` is the matter whose tree holds `path` (what [`Surroundings::set_aside`] is asked
/// for), not always the launching one: a launch that applies another matter's fence sets a child
/// in that matter's tree aside into that matter's set-aside, under its lock (S15 `PR3b` round 6).
///
/// [`Surroundings::set_aside`]: crate::windows::Surroundings::set_aside
#[cfg(windows)]
pub fn set_aside_path(
    data_dir: &Path,
    matter: &MatterId,
    path: &Path,
    reason: Reason,
    now: SystemTime,
) -> Result<Entry, crate::Error> {
    run::set_aside_path(&ntfs::Ntfs, data_dir, matter, path, reason, now)
        .map_err(run::Refused::into_error)
}

/// Every item set aside from the matter and not restored, oldest first.
#[cfg(windows)]
pub fn list_set_aside(data_dir: &Path, matter: &MatterId) -> Result<Vec<Entry>, crate::Error> {
    run::list(&ntfs::Ntfs, data_dir, matter).map_err(run::Refused::into_error)
}

/// Moves `entry`'s item back to where it was, only if that name is free, and drops it from the
/// manifest. Never overwrites: a taken name is [`SetAsideFailure::NameTaken`], an item no
/// longer there (or no longer listed) is [`SetAsideFailure::Gone`]. PR6 passes back the
/// manifest's own `Entry`, as [`list_set_aside`] gave it, never one simplified for display: its
/// paths may be verbatim (`\\?\C:\…`), and the item is found by them. The item restored is
/// then handed to the desktop user when a pool user the machine's record names owns it, as
/// forget hands back a pool user's item, so a restored agent's `CLAUDE.md` comes back the
/// lawyer's (S15 `PR4a2`); a hand-back that fails, or a record unreadable or empty, is logged,
/// and the next launch hands it back before its deny. A `.git`, or an item inside one, is never
/// put back ([`SetAsideFailure::GitFolder`]): handed back it would be the lawyer's, and git
/// trusts a lawyer's `.git`. Nor is a `config` or `hooks` set aside from a folder a launch
/// `commondir` named, nor a link leading out of the matter ([`SetAsideFailure::LinkOut`]).
#[cfg(windows)]
pub fn restore_set_aside(data_dir: &Path, entry: &Entry) -> Result<(), SetAsideFailure> {
    run::restore(&ntfs::Ntfs, data_dir, entry).map_err(|refused| refused.failure)
}

/// The outer runner's live watch over one watched folder (`runner/git_watch.rs`): each git
/// repository or outbound link made during the session moved aside at once, by the
/// after-session pass's own move (`live.rs`).
#[cfg(windows)]
pub(in crate::windows) struct GitGuard(live::Watched);

#[cfg(windows)]
impl GitGuard {
    /// The watch over `matter`'s folder `root`, knowing the git parts its record holds.
    pub(in crate::windows) fn new(root: &Path, data_dir: &Path, matter: &MatterId) -> GitGuard {
        GitGuard(live::Watched::new(&ntfs::Ntfs, root, data_dir, matter))
    }

    /// The watch over `matter`'s notes folder `root`, knowing the git parts the notes record
    /// holds (the launch pass's, [`before_launch`]); what it moves goes to `matter`'s set-aside.
    pub(in crate::windows) fn notes(root: &Path, data_dir: &Path, matter: &MatterId) -> GitGuard {
        GitGuard(live::Watched::new_notes(
            &ntfs::Ntfs,
            root,
            data_dir,
            matter,
        ))
    }

    /// The folder watched.
    pub(in crate::windows) fn root(&self) -> &Path {
        self.0.root()
    }

    /// Checks the paths the watch reported made or renamed, relative to the root.
    pub(in crate::windows) fn changed(
        &self,
        paths: &[PathBuf],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        self.0.changed(&ntfs::Ntfs, paths, now, hooks)
    }

    /// Checks the paths the watch reported turned into reparse points, relative to the root.
    pub(in crate::windows) fn relinked(
        &self,
        paths: &[PathBuf],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        self.0.relinked(&ntfs::Ntfs, paths, now, hooks)
    }

    /// Scans the whole folder for what the watch moves.
    pub(in crate::windows) fn rescan(&self, now: SystemTime, hooks: &impl Hooks) -> Live {
        self.0.rescan(&ntfs::Ntfs, now, hooks)
    }

    /// Tries again what a check couldn't move.
    pub(in crate::windows) fn retry(
        &self,
        failed: &[Failed],
        now: SystemTime,
        hooks: &impl Hooks,
    ) -> Live {
        self.0.retry(&ntfs::Ntfs, failed, now, hooks)
    }
}

#[cfg(test)]
pub(super) mod tests;
// Two attributes, as in `windows.rs`: clippy's test allowances look for `cfg(test)`.
#[cfg(test)]
#[cfg(windows)]
mod ntfs_tests;
