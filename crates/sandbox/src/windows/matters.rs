//! The matters' file, `<data_dir>\sandbox\matters.json`: for each matter, its own random SID
//! and the pool user it holds.
//!
//! A matter's SID is the restricting SID that keeps one matter's agent out of another's folders
//! (S15, "The token"). It is made at the matter's first launch, from the OS's randomness, and kept
//! for good: nothing here removes one, and only `forget_matter` (S15 PR3, S15a) will. A SID is
//! never handed to a second matter, and never one of this machine's accounts or their domain
//! ([`Reserved`]).
//!
//! A matter's pool user (design B, D9) is assigned by [`MatterSids::assign`] under the reuse rule
//! in [`pool::assign`](super::pool::assign), and kept until [`MatterSids::release`] after a full
//! forget. A released row stays, naming the user and when it was released: the pick order and
//! the open-session check read it. The release also marks the user as owed a profile reset: its
//! profile, `LocalLow` and hive may hold what the old matter's agent wrote there (its
//! `HKCU\Software\AppDataLow` is Low-writable), so no other matter gets it until the elevated
//! set-up has deleted and remade its profile and cleared the mark ([`MatterSids::reset_done`]).
//!
//! The file is version 3 (`file.rs`); a version 1 or 2 file is read and upgraded on the next
//! write.
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use clauseo_protocol::MatterId;

use super::ledger::Ledger;
use super::pool::assign::{self, Assign, OpenSessions, Pick};
use super::sids::{Reserved, Sid};
use super::write;
use crate::SetupError;

mod file;
mod set_aside;

use file::Contents;
pub use file::{MatterRow, Rows};

/// The folder under the data folder that holds the sandbox's records.
pub const SANDBOX_DIR: &str = "sandbox";
/// The matters' file, in [`SANDBOX_DIR`].
pub const MATTERS_FILE: &str = "matters.json";
/// The lock file every change of [`MATTERS_FILE`] holds, in [`SANDBOX_DIR`].
pub const MATTERS_LOCK: &str = "matters.lock";

/// The matters of one data folder: their SIDs and pool users.
#[derive(Debug)]
pub struct MatterSids {
    data_dir: PathBuf,
    folder: PathBuf,
    reserved: Reserved,
    rows: Rows,
    /// The pool users owed a profile reset.
    needs_reset: BTreeSet<Sid>,
    /// How long a change waits for `matters.lock`; `None` is [`write::LOCK_WAIT`].
    lock_wait: Option<Duration>,
    /// Reads an owner for the set-aside walk (`set_aside.rs`); a fake in the tests.
    owner_of: set_aside::OwnerOf,
}

/// The SID of `matter` in `data_dir`'s `matters.json`, made and saved first if the matter has
/// none ([`MatterSids::sid_for`]).
pub fn matter_sid(data_dir: &Path, matter: &MatterId) -> Result<Sid, SetupError> {
    MatterSids::load(data_dir)?.sid_for(matter)
}

impl MatterSids {
    /// Loads `<data_dir>\sandbox\matters.json` against this machine's accounts
    /// ([`Reserved::of_this_machine`] on Windows; none elsewhere, where only tests load it).
    pub fn load(data_dir: &Path) -> Result<MatterSids, SetupError> {
        #[cfg(windows)]
        let reserved = Reserved::of_this_machine()?;
        #[cfg(not(windows))]
        let reserved = Reserved::default();
        MatterSids::load_with(data_dir, reserved)
    }

    /// Loads `<data_dir>\sandbox\matters.json`, reading only; no file is no matters. A file
    /// that can't be read or parsed, is at a version other than 1 or 2, names a matter twice,
    /// has a row with neither a SID nor a pool user, holds a SID not of the matter form or one
    /// `reserved` refuses, holds one SID twice, or has one pool user held by two matters is an
    /// error, and is left as it is: rewriting it would lose matters' SIDs and pool users, and
    /// with them the way to remove their permission entries.
    pub fn load_with(data_dir: &Path, reserved: Reserved) -> Result<MatterSids, SetupError> {
        let folder = data_dir.join(SANDBOX_DIR);
        let read = file::read(&folder.join(MATTERS_FILE), &reserved)?;
        Ok(MatterSids {
            data_dir: data_dir.to_owned(),
            folder,
            reserved,
            rows: read.rows,
            needs_reset: read.needs_reset,
            lock_wait: None,
            owner_of: set_aside::owner_of,
        })
    }

    /// Waits up to `wait` for `matters.lock` instead of [`write::LOCK_WAIT`] (10 s).
    pub fn with_lock_wait(mut self, wait: Duration) -> MatterSids {
        self.lock_wait = Some(wait);
        self
    }

    /// The SID `matter` had when this was loaded or last changed, if any.
    pub fn get(&self, matter: &MatterId) -> Option<&Sid> {
        self.rows.get(matter).and_then(|row| row.sid.as_ref())
    }

    /// The pool user `matter` held when this was loaded or last changed, if any: none once
    /// released.
    pub fn pool_user(&self, matter: &MatterId) -> Option<&Sid> {
        self.rows.get(matter).and_then(MatterRow::holds)
    }

    /// Every matter's row, as loaded or last changed.
    pub fn rows(&self) -> &Rows {
        &self.rows
    }

    /// The pool users owed a profile reset, as loaded or last changed.
    pub fn needs_reset(&self) -> &BTreeSet<Sid> {
        &self.needs_reset
    }

    /// The pool users the elevated set-up may reset now, as loaded or last changed: those owed
    /// a reset that no matter holds. A user a matter holds is left marked for after its release.
    pub fn resets_due(&self) -> Vec<Sid> {
        let held: BTreeSet<&Sid> = self.rows.values().filter_map(MatterRow::holds).collect();
        let due = self.needs_reset.iter().filter(|user| !held.contains(user));
        due.cloned().collect()
    }

    /// The users of `pool` that [`MatterSids::assign`] would hold back once the elevated set-up
    /// has reset `resets`, the users its run acts on (read once, before, through
    /// [`MatterSids::resets_due`]): the set-up's top-up counts the rest free
    /// (`setup/top_up.rs`). The same rule as `assign` (`assign::held_back_each`): the rows as
    /// loaded, `ledger` (grants and hand-backs owed), the set-aside owners, the open sessions
    /// and every mark but those of `resets`, so a release marked since that read stays held. A
    /// ledger, or open sessions whose lock can't be taken, hold every user, as an unreadable
    /// set-aside manifest does; an unreadable open-session record counts its matter open.
    pub fn held_after_resets(
        &self,
        pool: &[Sid],
        ledger: io::Result<Ledger>,
        resets: &[Sid],
    ) -> Vec<Sid> {
        let each = self
            .held_after_resets_each(pool, ledger, resets)
            .into_iter();
        each.map(|(user, _why)| user).collect()
    }

    /// [`MatterSids::held_after_resets`], each user with why (`None`: an input couldn't be read,
    /// which holds every user), for the top-up's log.
    pub fn held_after_resets_each(
        &self,
        pool: &[Sid],
        ledger: io::Result<Ledger>,
        resets: &[Sid],
    ) -> Vec<(Sid, Option<assign::Why>)> {
        let (Ok(ledger), Ok(open)) = (ledger, self.open_sessions()) else {
            return pool.iter().map(|user| (user.clone(), None)).collect();
        };
        let marks = self
            .needs_reset
            .iter()
            .filter(|user| !resets.contains(user));
        let needs_reset: BTreeSet<Sid> = marks.cloned().collect();
        let set_aside = set_aside::set_aside_owners(&self.data_dir, self.owner_of);
        let held = assign::Holds {
            ledger: &ledger,
            set_aside: &set_aside,
            open: &open,
            needs_reset: &needs_reset,
        };
        let each = assign::held_back_each(pool, &self.rows, &held).into_iter();
        each.map(|(user, why)| (user, Some(why))).collect()
    }

    /// Clears the reset mark of each of `users`, once the elevated set-up has deleted and remade
    /// its profile and closed its `LocalLow` again. Inside [`MatterSids::change`]'s lock and
    /// fresh read, so a mark another release added meanwhile stays; a user a matter holds keeps
    /// its mark, as its profile wasn't reset. Which marks were cleared.
    pub fn reset_done(&mut self, users: &[Sid]) -> Result<Vec<Sid>, SetupError> {
        self.change(|contents, _| {
            let held: BTreeSet<Sid> = contents
                .rows
                .values()
                .filter_map(MatterRow::holds)
                .cloned()
                .collect();
            let cleared: Vec<Sid> = users
                .iter()
                .filter(|user| !held.contains(*user) && contents.needs_reset.remove(*user))
                .cloned()
                .collect();
            let changed = !cleared.is_empty();
            Ok((cleared, changed))
        })
    }

    /// How many matters are listed.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no matter is listed.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The listed matters with an open session, each read under its set-aside lock (an
    /// unreadable record counting as open): the set-up's refusal to reset passwords while a
    /// session runs (D-B6, S15 `PR4h`). A lock that can't be taken is the error.
    pub fn open_sessions(&self) -> Result<OpenSessions, SetupError> {
        let mut open = Vec::new();
        for matter in self.rows.keys() {
            if session_open(&self.data_dir, matter)? {
                open.push(*matter);
            }
        }
        Ok(OpenSessions::new(open))
    }

    /// The SID of `matter`. Inside [`MatterSids::change`]'s lock and fresh read, so a matter
    /// another instance added since this one loaded is kept. A matter without a SID gets a new
    /// random one that no matter holds, no row names as a pool user and [`Reserved`] admits,
    /// and the file is saved with it before it is returned; if the save fails, the matter has no
    /// SID.
    pub fn sid_for(&mut self, matter: &MatterId) -> Result<Sid, SetupError> {
        self.change(|contents, reserved| {
            let rows = &mut contents.rows;
            if let Some(sid) = rows.get(matter).and_then(|row| row.sid.clone()) {
                return Ok((sid, false));
            }
            let taken = |sid: &Sid| {
                rows.values()
                    .any(|row| row.sid.as_ref() == Some(sid) || row.pool_user.as_ref() == Some(sid))
            };
            let sid = loop {
                let sid = Sid::random_matter()
                    .map_err(|why| SetupError::Failed(format!("matter SID: {why}")))?;
                if reserved.admits(&sid) && !taken(&sid) {
                    break sid;
                }
            };
            rows.entry(*matter).or_default().sid = Some(sid.clone());
            Ok((sid, true))
        })
    }

    /// The pool user of `matter`, from `pool` (the machine record's `PoolSids`). Inside
    /// [`MatterSids::change`]'s lock and fresh read. A matter that holds a user still in `pool`
    /// gets it back at once: nothing else is read and nothing written, so a launch pays no
    /// ledger read and no set-aside walk. Otherwise `ledger` is called there to read the ACL
    /// ledger and the set-aside manifests and trees are read (`set_aside.rs`), so an entry,
    /// hand-back or item recorded before the lock was taken is seen. Then the user
    /// [`pool::assign`](super::pool::assign) picks, by its reuse rule, is saved in the matter's
    /// row before it is returned; if the save fails, the matter holds none. A released matter
    /// asking again gets a fresh pick, and its old user goes to `held_before`. A user owed a
    /// profile reset isn't picked. No free user is [`Assign::PoolFull`]. `open` is the caller's
    /// read; see [`OpenSessions`] for why a stale one is safe.
    ///
    /// The caller marks the launch's session open (`before_launch`) **before** calling this, so
    /// no [`MatterSids::release`] can take the user between this and the session. A new pick
    /// for a matter whose set-aside record (read under its set-aside lock) doesn't list
    /// `session` as open is refused with [`Assign::Records`]; a matter that keeps its user needs
    /// none. A pool user serves one live session at a time: while the record lists another open
    /// session of the matter, or can't be read, the launch is refused with [`Assign::Busy`]
    /// ([`assign::busy`]), whether the matter keeps its user or would get a new one.
    pub fn assign(
        &mut self,
        matter: &MatterId,
        session: &str,
        pool: &[Sid],
        ledger: impl FnOnce() -> io::Result<Ledger>,
        open: &OpenSessions,
    ) -> Result<Sid, Assign> {
        let data_dir = self.data_dir.clone();
        let owner_of = self.owner_of;
        self.change(|contents, _| {
            let Contents { rows, needs_reset } = contents;
            let sessions = sessions_open(&data_dir, matter)?;
            if assign::busy(sessions.as_ref(), session) {
                return Err(Assign::Busy(*matter));
            }
            // The launch path: a matter keeping its user reads neither the ledger nor the
            // set-aside trees.
            if let Some(user) = rows.get(matter).and_then(MatterRow::holds)
                && pool.contains(user)
            {
                return Ok((user.clone(), false));
            }
            let ledger =
                ledger().map_err(|error| SetupError::Failed(format!("the ACL ledger: {error}")))?;
            let set_aside = set_aside::set_aside_owners(&data_dir, owner_of);
            let held = assign::Holds {
                ledger: &ledger,
                set_aside: &set_aside,
                open,
                needs_reset,
            };
            match assign::decide(matter, rows, pool, &held)? {
                Pick::Kept(user) => Ok((user, false)),
                Pick::New(user) => {
                    if !sessions.is_some_and(|open| open.contains(session)) {
                        return Err(Assign::Records(SetupError::Failed(format!(
                            "{matter} has no open session: before_launch marks it before assign"
                        ))));
                    }
                    let row = rows.entry(*matter).or_default();
                    if let Some(old) = row.pool_user.take()
                        && !row.held_before.contains(&old)
                    {
                        row.held_before.push(old);
                    }
                    row.pool_user = Some(user.clone());
                    row.released_at = None;
                    Ok((user, true))
                }
            }
        })
    }

    /// Gives back `matter`'s pool user, after a full forget (`forget_matter` with no hand-back
    /// left owed): its row keeps the user, with `now` (whole seconds since the Unix epoch; 0
    /// for a time before it) as `released_at`, and the user is marked as owed a profile reset,
    /// in the same save, so no other matter gets it before the elevated set-up resets it.
    /// Inside [`MatterSids::change`]'s lock and fresh read. Refused while a session of the
    /// matter is open, read under the matter's set-aside lock (`set_aside::under_lock`, an
    /// unreadable record counting as open), so a user is never released from under a live
    /// session, nor between a launch's `assign` and its session, as the launch marks the session
    /// first (see [`MatterSids::assign`]). The lock order is `matters.lock`, then a set-aside
    /// lock, never the reverse. Whether a user was released: a matter with none, or released
    /// already, is left as it is.
    pub fn release(&mut self, matter: &MatterId, now: SystemTime) -> Result<bool, SetupError> {
        let at = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let data_dir = self.data_dir.clone();
        self.change(|contents, _| {
            let Some(row) = contents.rows.get_mut(matter) else {
                return Ok((false, false));
            };
            let Some(user) = row.holds().cloned() else {
                return Ok((false, false));
            };
            if session_open(&data_dir, matter)? {
                return Err(SetupError::Failed(format!(
                    "{matter} has an open session; its sandbox user is kept"
                )));
            }
            row.released_at = Some(at);
            contents.needs_reset.insert(user);
            Ok((true, true))
        })
    }

    /// One change of the file. Makes the sandbox folder if missing (`write::make_sandbox_dir`),
    /// checks `matters.lock` (`write::check_owned_file`) and holds it (an exclusive OS file
    /// lock, so no other process or instance changes the file meanwhile; waiting at most 10 s,
    /// or [`MatterSids::with_lock_wait`], then an error naming the lock), and checks and reads
    /// the file again inside it. `change` gets what was read and the reservations, and returns
    /// its result and whether it changed them; changes are saved, at version 3, and kept here
    /// only once saved.
    fn change<T, E: From<SetupError>>(
        &mut self,
        change: impl FnOnce(&mut Contents, &Reserved) -> Result<(T, bool), E>,
    ) -> Result<T, E> {
        let failed =
            |why: io::Error| SetupError::Failed(format!("{}: {why}", self.folder.display()));
        write::make_sandbox_dir(&self.folder).map_err(failed)?;
        let lock = self.folder.join(MATTERS_LOCK);
        write::check_owned_file(&lock).map_err(failed)?;
        let _held = match self.lock_wait {
            None => write::lock_exclusive(&lock),
            Some(wait) => write::lock_exclusive_for(&lock, wait),
        }
        .map_err(|error| SetupError::Failed(format!("matter SIDs: {error}")))?;
        // A lock file the open above just made gets the same explicit DACL.
        write::check_owned_file(&lock).map_err(failed)?;
        let path = self.folder.join(MATTERS_FILE);
        write::check_owned_file(&path).map_err(failed)?;
        let read = file::read(&path, &self.reserved)?;
        self.rows.clone_from(&read.rows);
        self.needs_reset.clone_from(&read.needs_reset);
        let mut contents = read;
        let (value, changed) = change(&mut contents, &self.reserved)?;
        if changed {
            file::save(&path, &contents)?;
            self.rows = contents.rows;
            self.needs_reset = contents.needs_reset;
            // The save put a new file in place; it gets the same explicit DACL.
            write::check_owned_file(&path).map_err(failed)?;
        }
        Ok(value)
    }
}

/// Whether a session of `matter` is open, read under its set-aside lock; an unreadable record
/// counts as open.
#[cfg(any(windows, test))]
fn session_open(data_dir: &Path, matter: &MatterId) -> Result<bool, SetupError> {
    super::set_aside::under_lock(data_dir, matter, |open| open)
        .map_err(|error| SetupError::Failed(format!("{matter}'s set-aside lock: {error}")))
}

/// The sessions of `matter` its set-aside record lists as open, read under its set-aside lock;
/// `None` when the record can't be read.
#[cfg(any(windows, test))]
fn sessions_open(
    data_dir: &Path,
    matter: &MatterId,
) -> Result<Option<BTreeSet<String>>, SetupError> {
    super::set_aside::open_sessions_of(data_dir, matter)
        .map_err(|error| SetupError::Failed(format!("{matter}'s set-aside lock: {error}")))
}

/// Off Windows, outside the tests, there are no set-aside records: refused, failing closed.
#[cfg(not(any(windows, test)))]
fn sessions_open(
    _data_dir: &Path,
    matter: &MatterId,
) -> Result<Option<BTreeSet<String>>, SetupError> {
    Err(SetupError::Failed(format!(
        "{matter}: no set-aside record on this OS"
    )))
}

/// Off Windows, outside the tests, there are no set-aside records: refused, failing closed.
#[cfg(not(any(windows, test)))]
fn session_open(_data_dir: &Path, matter: &MatterId) -> Result<bool, SetupError> {
    Err(SetupError::Failed(format!(
        "{matter}: no set-aside record on this OS"
    )))
}

#[cfg(test)]
mod tests;
