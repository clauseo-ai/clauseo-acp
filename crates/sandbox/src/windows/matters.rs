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
//! the open-session check read it.
//!
//! The file is version 2 (`file.rs`); a version 1 file is read and upgraded on the next write.
//!
//! Built on every OS so its tests run everywhere; only Windows uses it.

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
        let rows = file::read(&folder.join(MATTERS_FILE), &reserved)?;
        Ok(MatterSids {
            data_dir: data_dir.to_owned(),
            folder,
            reserved,
            rows,
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

    /// How many matters are listed.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Whether no matter is listed.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The SID of `matter`. Inside [`MatterSids::change`]'s lock and fresh read, so a matter
    /// another instance added since this one loaded is kept. A matter without a SID gets a new
    /// random one that no matter holds, no row names as a pool user and [`Reserved`] admits,
    /// and the file is saved with it before it is returned; if the save fails, the matter has no
    /// SID.
    pub fn sid_for(&mut self, matter: &MatterId) -> Result<Sid, SetupError> {
        self.change(|rows, reserved| {
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
    /// asking again gets a fresh pick, and its old user goes
    /// to `held_before`. No free user is [`Assign::PoolFull`]. `open` is the caller's read;
    /// see [`OpenSessions`] for why a stale one is safe.
    ///
    /// The caller marks the launch's session open (`before_launch`) **before** calling this, so
    /// no [`MatterSids::release`] can take the user between this and the session. A new pick
    /// for a matter with no open session in its set-aside record (read under its set-aside lock)
    /// is refused with [`Assign::Records`]; a matter that keeps its user needs none (set-up reads
    /// registered matters' users with no session).
    pub fn assign(
        &mut self,
        matter: &MatterId,
        pool: &[Sid],
        ledger: impl FnOnce() -> io::Result<Ledger>,
        open: &OpenSessions,
    ) -> Result<Sid, Assign> {
        let data_dir = self.data_dir.clone();
        let owner_of = self.owner_of;
        self.change(|rows, _| {
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
            match assign::decide(matter, rows, pool, &ledger, &set_aside, open)? {
                Pick::Kept(user) => Ok((user, false)),
                Pick::New(user) => {
                    if !session_open(&data_dir, matter)? {
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
    /// for a time before it) as `released_at`. Inside [`MatterSids::change`]'s lock and fresh
    /// read. Refused while a session of the matter is open, read under the matter's set-aside
    /// lock (`set_aside::under_lock`, an unreadable record counting as open), so a user is
    /// never released from under a live session, nor between a launch's `assign` and its
    /// session, as the launch marks the session first (see [`MatterSids::assign`]). The lock
    /// order is `matters.lock`, then a set-aside lock, never the reverse. Whether a user was
    /// released: a matter with none, or released already, is left as it is.
    pub fn release(&mut self, matter: &MatterId, now: SystemTime) -> Result<bool, SetupError> {
        let at = now
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        let data_dir = self.data_dir.clone();
        self.change(|rows, _| {
            let Some(row) = rows.get_mut(matter).filter(|row| row.holds().is_some()) else {
                return Ok((false, false));
            };
            if session_open(&data_dir, matter)? {
                return Err(SetupError::Failed(format!(
                    "{matter} has an open session; its sandbox user is kept"
                )));
            }
            row.released_at = Some(at);
            Ok((true, true))
        })
    }

    /// One change of the file. Makes the sandbox folder if missing (`write::make_sandbox_dir`),
    /// checks `matters.lock` (`write::check_owned_file`) and holds it (an exclusive OS file
    /// lock, so no other process or instance changes the file meanwhile; waiting at most 10 s,
    /// or [`MatterSids::with_lock_wait`], then an error naming the lock), and checks and reads
    /// the file again inside it. `change` gets the rows read and the reservations, and returns
    /// its result and whether it changed the rows; changed rows are saved, at version 2, and
    /// kept here only once saved.
    fn change<T, E: From<SetupError>>(
        &mut self,
        change: impl FnOnce(&mut Rows, &Reserved) -> Result<(T, bool), E>,
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
        self.rows = read.clone();
        let mut rows = read;
        let (value, changed) = change(&mut rows, &self.reserved)?;
        if changed {
            file::save(&path, &rows)?;
            self.rows = rows;
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

/// Off Windows, outside the tests, there are no set-aside records: refused, failing closed.
#[cfg(not(any(windows, test)))]
fn session_open(_data_dir: &Path, matter: &MatterId) -> Result<bool, SetupError> {
    Err(SetupError::Failed(format!(
        "{matter}: no set-aside record on this OS"
    )))
}

#[cfg(test)]
mod tests;
