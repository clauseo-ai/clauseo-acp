//! The ACL ledger, `<data_dir>\sandbox\acl-ledger.json`: every permission entry tier B writes,
//! so set-up can remove them and a matter can be forgotten (S15's D4, R6).
//!
//! It is write-ahead. An entry is recorded `pending` and the file replaced before its permission
//! entry is written; once it is written, the entry is marked `applied`. At start, [`reconcile`]
//! keeps each `pending` entry whose permission entry is there (marking it `applied`) and drops
//! the rest. So every permission entry written is listed, and no `applied` entry lacks one.
//!
//! Every change is a read-modify-write under the exclusive OS lock on `acl-ledger.lock` beside
//! the file (`write::lock_exclusive_for`, the records' bounded wait): the file is read again
//! inside the lock, changed, and replaced whole through a uniquely named temporary file and a
//! rename (`write::write_atomic`), so two processes never lose each other's entries and a failed
//! write leaves the old file as it was. Entry numbers come from a counter stored in the file and
//! advanced under that lock, so they are unique across launches and processes and never reused.
//! The folder is made, or checked and secured, by `write::make_sandbox_dir` (its protected DACL,
//! its owner), and the lock file and the ledger by `write::check_owned_file`, as the matter SIDs'
//! files are. Nothing is written unless the entries change: a launch refused by the gate never
//! reaches the ledger, and a missing ledger stays missing until an entry is recorded.
//!
//! Only an [`AdmittedPlan`] is recorded, and a rollback ([`Ledger::drop_entries`]) drops only the
//! entries its own plan added.
//!
//! Entries are not trusted on load. One this version can't read (an older shape, a newer kind),
//! one whose SID is neither a trusted SID given to [`Ledger::open`] (the pool users and their
//! group) nor a matter-form SID outside the reserved accounts and domains (`sids::Reserved`), a
//! label item naming anything but the Low level, and one whose path isn't absolute, are kept as
//! they are, written back unchanged, and never acted on or handed out. A file from a newer format
//! version, or one that isn't JSON, is refused with `InvalidData` and left alone.
//!
//! [`reconcile`]: Ledger::reconcile

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use super::admit::PathKind;
use super::matters::SANDBOX_DIR;
use super::sids::{Reserved, Sid};
use super::write;

mod file;

use file::{Known, Slot, State, Trust, exists, invalid, read_state, render};

/// The ledger's file name in `<data_dir>\sandbox`.
pub const LEDGER_FILE: &str = "acl-ledger.json";

/// The lock file every change of [`LEDGER_FILE`] holds, beside it.
pub const LEDGER_LOCK: &str = "acl-ledger.lock";

/// The format version this code writes and the newest it reads.
pub const LEDGER_VERSION: u64 = 1;

/// `<data_dir>\sandbox\acl-ledger.json`.
pub fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SANDBOX_DIR).join(LEDGER_FILE)
}

/// Whether an entry's permission entry is known to be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Recorded before the permission entry is written; it may or may not be there.
    Pending,
    /// The permission entry was written.
    Applied,
}

/// An entry's number, unique in its ledger and never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryId(pub u64);

/// One permission entry, listed. Read from the ledger only; never made outside this crate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Entry {
    /// Its number.
    pub id: EntryId,
    /// The matter it was written for.
    pub matter: MatterId,
    /// Pending or applied.
    pub state: EntryState,
    /// Where.
    pub path: PathBuf,
    /// Whom.
    pub sid: Sid,
    /// What.
    pub rights: Rights,
    /// How far down.
    pub inherit: Inherit,
    /// Which of the policy's paths.
    pub kind: PathKind,
}

impl Entry {
    /// The permission entry, as planned.
    pub fn grant(&self) -> Grant {
        Grant {
            path: self.path.clone(),
            sid: self.sid.clone(),
            rights: self.rights,
            inherit: self.inherit,
            kind: self.kind,
        }
    }

    /// Whether this entry lists `grant` for `matter`.
    fn lists(&self, matter: MatterId, grant: &Grant) -> bool {
        self.matter == matter
            && self.path == grant.path
            && self.sid == grant.sid
            && self.rights == grant.rights
            && self.inherit == grant.inherit
            && self.kind == grant.kind
    }
}

/// The entries [`Ledger::mark_applied`] was asked to mark that the file no longer lists; the
/// inner error of its `NotFound` (`io::Error::get_ref` and `downcast_ref`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the ACL ledger no longer lists {} of this launch's entries", .0.len())]
pub struct MissingEntries(pub Vec<EntryId>);

/// What [`Ledger::reconcile`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reconciled {
    /// Pending entries whose permission entry was there, now applied.
    pub applied: usize,
    /// Pending entries whose permission entry wasn't there, dropped.
    pub dropped: usize,
    /// Pending entries the check failed on, kept pending.
    pub unchecked: usize,
}

/// What [`Ledger::forget_matter`] did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forgotten {
    /// Entries whose permission entry was removed, now dropped.
    pub removed: usize,
    /// Entries whose removal failed, kept for the next try.
    pub kept: usize,
}

/// The ledger: its path, whom its entries may name, and the contents last read. Each change
/// reads the file again under its lock, so the contents here are a cache.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    trust: Trust,
    state: State,
}

impl Ledger {
    /// Reads the ledger at `path` (see [`ledger_path`]). `trusted` are the pool users' SIDs and
    /// their group's, and `reserved` the accounts and domains no matter SID may be in
    /// (`Reserved::of_this_machine` on Windows): an entry naming any other SID is set aside. A
    /// missing file is an empty ledger. A newer format version or a file that isn't a JSON
    /// object is `InvalidData`.
    pub fn open(
        path: impl Into<PathBuf>,
        trusted: &[Sid],
        reserved: &Reserved,
    ) -> io::Result<Ledger> {
        let path = path.into();
        let trust = Trust {
            trusted: trusted.to_vec(),
            reserved: reserved.clone(),
        };
        let state = read_state(&path, &trust)?;
        Ok(Ledger { path, trust, state })
    }

    /// The file this ledger replaces.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The entries this version reads and trusts, in file order, as last read.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.state.entries()
    }

    /// How many entries are set aside: unread or untrusted. They are kept and never acted on.
    pub fn set_aside_entries(&self) -> usize {
        self.state
            .slots
            .iter()
            .filter(|slot| matches!(slot, Slot::SetAside(_)))
            .count()
    }

    /// `matter`'s trusted entries, as last read, for removal.
    pub fn matter_entries(&self, matter: MatterId) -> Vec<Entry> {
        self.entries()
            .filter(|entry| entry.matter == matter)
            .cloned()
            .collect()
    }

    /// The write-ahead step, under the lock: lists each of `plan`'s grants as `pending`, then
    /// replaces the file, before any permission entry is written. A grant already listed for the
    /// matter, pending or applied, keeps its entry. The plan learns each grant's entry
    /// ([`AdmittedPlan::entries`]) and which ones it added ([`AdmittedPlan::created`]). A plan
    /// already recorded is refused (`InvalidInput`). The file is untouched when every grant was
    /// listed already, or when the write fails.
    pub fn record_pending(&mut self, plan: &mut AdmittedPlan) -> io::Result<()> {
        if !plan.entries.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "this plan was recorded already",
            ));
        }
        let matter = plan.matter();
        let (entries, created) = self.update(true, |state| {
            let mut entries = Vec::with_capacity(plan.grants().len());
            let mut created = Vec::new();
            for grant in plan.grants() {
                if let Some(id) = state
                    .entries()
                    .find(|entry| entry.lists(matter, grant))
                    .map(|entry| entry.id)
                {
                    entries.push(id);
                    continue;
                }
                let id = EntryId(state.next_id);
                state.next_id = state
                    .next_id
                    .checked_add(1)
                    .ok_or_else(|| invalid("the ACL ledger ran out of entry numbers"))?;
                state.slots.push(Slot::Known(Known {
                    entry: Entry {
                        id,
                        matter,
                        state: EntryState::Pending,
                        path: grant.path.clone(),
                        sid: grant.sid.clone(),
                        rights: grant.rights,
                        inherit: grant.inherit,
                        kind: grant.kind,
                    },
                    extra: Map::new(),
                }));
                entries.push(id);
                created.push(id);
            }
            let changed = !created.is_empty();
            Ok((changed, (entries, created)))
        })?;
        plan.entries = entries;
        plan.created = created;
        Ok(())
    }

    /// Marks the entries `ids` `applied`, once their permission entries are written. Only ids of
    /// `plan`'s own entries count; others are ignored. When one of them is no longer in the file
    /// (another process reconciled it away), nothing is marked and the error is `NotFound`
    /// carrying [`MissingEntries`]: the caller must not count that permission entry as listed.
    pub fn mark_applied(&mut self, plan: &AdmittedPlan, ids: &[EntryId]) -> io::Result<()> {
        let ids: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| plan.entries.contains(id))
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        self.update(false, |state| {
            let missing: Vec<EntryId> = ids
                .iter()
                .copied()
                .filter(|id| !state.entries().any(|entry| entry.id == *id))
                .collect();
            if !missing.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    MissingEntries(missing),
                ));
            }
            let mut changed = false;
            for slot in &mut state.slots {
                if let Slot::Known(known) = slot
                    && ids.contains(&known.entry.id)
                    && known.entry.state == EntryState::Pending
                {
                    known.entry.state = EntryState::Applied;
                    changed = true;
                }
            }
            Ok((changed, ()))
        })
    }

    /// Drops the entries `ids`, once their permission entries are known to be gone: a launch's
    /// rollback. Only entries `plan` itself added count; an entry an earlier launch listed, or
    /// another launch's, is never dropped here.
    pub fn drop_entries(&mut self, plan: &mut AdmittedPlan, ids: &[EntryId]) -> io::Result<()> {
        let ids: Vec<EntryId> = ids
            .iter()
            .copied()
            .filter(|id| plan.created.contains(id))
            .collect();
        if ids.is_empty() {
            return Ok(());
        }
        self.update(false, |state| {
            let before = state.slots.len();
            state.slots.retain(
                |slot| !matches!(slot, Slot::Known(known) if ids.contains(&known.entry.id)),
            );
            Ok((state.slots.len() != before, ()))
        })?;
        plan.created.retain(|id| !ids.contains(id));
        Ok(())
    }

    /// At start, under the lock: asks `present` about each trusted `pending` entry. One whose
    /// permission entry is there is marked `applied`; one whose permission entry isn't is
    /// dropped; one `present` fails on stays pending for the next start. Applied entries and set
    /// aside ones are left as they are.
    pub fn reconcile(
        &mut self,
        mut present: impl FnMut(&Entry) -> io::Result<bool>,
    ) -> io::Result<Reconciled> {
        self.update(false, |state| {
            let mut done = Reconciled::default();
            let mut slots = Vec::with_capacity(state.slots.len());
            for slot in std::mem::take(&mut state.slots) {
                let Slot::Known(mut known) = slot else {
                    slots.push(slot);
                    continue;
                };
                if known.entry.state != EntryState::Pending {
                    slots.push(Slot::Known(known));
                    continue;
                }
                match present(&known.entry) {
                    Ok(true) => {
                        known.entry.state = EntryState::Applied;
                        slots.push(Slot::Known(known));
                        done.applied += 1;
                    }
                    Ok(false) => done.dropped += 1,
                    Err(error) => {
                        // The kinds alone: the error's text may hold the path.
                        let (kind, error) = (known.entry.kind, error.kind());
                        tracing::warn!(%kind, %error, "a pending ACL entry wasn't checked");
                        slots.push(Slot::Known(known));
                        done.unchecked += 1;
                    }
                }
            }
            state.slots = slots;
            Ok((done.applied + done.dropped > 0, done))
        })
    }

    /// Forgets `matter`, under the lock: hands each of its trusted entries to `remove`, which
    /// removes the permission entry (one already gone counts as removed). Entries removed are
    /// dropped; entries `remove` failed on are kept for the next try. Other matters' entries and
    /// set aside ones are left as they are, and never handed to `remove`.
    pub fn forget_matter(
        &mut self,
        matter: MatterId,
        mut remove: impl FnMut(&Entry) -> io::Result<()>,
    ) -> io::Result<Forgotten> {
        self.update(false, |state| {
            let mut done = Forgotten::default();
            let mut slots = Vec::with_capacity(state.slots.len());
            for slot in std::mem::take(&mut state.slots) {
                match slot {
                    Slot::Known(known) if known.entry.matter == matter => {
                        match remove(&known.entry) {
                            Ok(()) => done.removed += 1,
                            Err(error) => {
                                let (kind, error) = (known.entry.kind, error.kind());
                                tracing::warn!(%kind, %error, "an ACL entry wasn't removed");
                                slots.push(Slot::Known(known));
                                done.kept += 1;
                            }
                        }
                    }
                    other @ (Slot::Known(_) | Slot::SetAside(_)) => slots.push(other),
                }
            }
            state.slots = slots;
            Ok((done.removed > 0, done))
        })
    }

    /// One read-modify-write. With `create` false and no ledger file, there is nothing to change:
    /// `change` runs on an empty ledger and nothing is made. Otherwise the folder is made or
    /// secured, the lock taken, the file read again, `change` run on what was read, and the file
    /// replaced if `change` says it changed something. The cache then matches the file: the new
    /// contents, or on a failed write what was read.
    fn update<R>(
        &mut self,
        create: bool,
        change: impl FnOnce(&mut State) -> io::Result<(bool, R)>,
    ) -> io::Result<R> {
        let folder = self
            .path
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the ledger has no folder"))?
            .to_owned();
        if !create && !exists(&self.path)? {
            let mut state = State::empty();
            let (_, result) = change(&mut state)?;
            self.state = State::empty();
            return Ok(result);
        }
        write::make_sandbox_dir(&folder)?;
        // As `MatterSids::sid_for` does: the lock file is checked before it is locked, and the
        // ledger under the lock (owned by the user, `SYSTEM` or `Administrators`, one link, no
        // reparse point, its entries reset to the folder's).
        let lock = folder.join(LEDGER_LOCK);
        write::check_owned_file(&lock)?;
        let _held = write::lock_exclusive_for(&lock, write::LOCK_WAIT)?;
        write::check_owned_file(&self.path)?;
        let fresh = read_state(&self.path, &self.trust)?;
        let mut next = fresh.clone();
        let outcome = change(&mut next);
        let (changed, result) = match outcome {
            Ok(done) => done,
            Err(error) => {
                self.state = fresh;
                return Err(error);
            }
        };
        if changed {
            if let Err(error) =
                render(&next).and_then(|bytes| write::write_atomic(&self.path, &bytes))
            {
                self.state = fresh;
                return Err(error);
            }
            self.state = next;
        } else {
            self.state = fresh;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
