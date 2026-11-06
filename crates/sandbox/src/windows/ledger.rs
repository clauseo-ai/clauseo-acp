//! The ACL ledger, `<data_dir>\sandbox\acl-ledger.json`: every permission entry tier B writes,
//! so set-up can remove them and a matter can be forgotten (S15's D4, R6).
//!
//! It is write-ahead. An entry is recorded `pending` and the file replaced before its permission
//! entry is written; once it is written and its tree walked, the entry is marked `applied`. At
//! start, [`reconcile`] walks each `pending` entry's tree again, then marks it `applied` if
//! its permission entry is there and drops it if not; a deny entry stays `pending` instead, so
//! its matter's next launch walks its tree strictly (S15 `PR3b` round 2, N4). Taking entries back
//! ([`Ledger::take_back`]) marks them `removing` on disk before any removal, so a crash partway
//! is never read as a launch that wrote them (round 4); [`reconcile`] finishes their removal. It
//! leaves a
//! permission entry another entry names: the same entry on the same path, or a Low label on
//! another name for the same file. A Low label's earlier label is never recorded when it is our
//! own label, and goes to the entry that shares the label when its own entry is dropped (N7). So
//! every permission entry written is listed, and no `applied` entry lacks one or its walk.
//!
//! A forget records the hand-back it owes each pool user of its matter, under the lock and before
//! any removal, and clears it only once every hand-back walk has succeeded (`owed.rs`, N5). While
//! one is owed, the matter's entries are never dropped by [`reconcile`].
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
//! entries its own plan added. A take-back that picks other matters' entries is the one change
//! that drops them: a sibling deny lifted from a folder that became a matter (S15 PR3, `apply`).
//!
//! Entries are not trusted on load. One this version can't read (an older shape, a newer kind),
//! one whose SID is neither a trusted SID given to [`Ledger::open`] (the pool users and their
//! group) nor a matter-form SID outside the reserved accounts and domains (`sids::Reserved`), a
//! label item naming anything but the Low level, and one whose path isn't absolute, are kept as
//! they are, written back unchanged, and never acted on or handed out. So is a hand-back owed
//! this version can't read or that names an untrusted SID, except that it still blocks the reuse
//! of the pool user it names. A file from a newer format
//! version, or one that isn't JSON, is refused with `InvalidData` and left alone.
//!
//! [`reconcile`]: super::apply::reconcile

use std::io;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::Map;

use super::acl_plan::{AdmittedPlan, Grant, Inherit, Rights};
use super::admit::PathKind;
use super::fence::{MatterFolders, plan_fence};
use super::matters::SANDBOX_DIR;
use super::sids::{Reserved, Sid};
use super::writer::Label;

mod file;
mod held;
mod outcome;
mod owed;
mod recorded;
mod settle;

use file::{Known, Slot, State, Trust, invalid, read_state};
#[cfg(test)]
pub(crate) use held::ByPath;
#[cfg(not(test))]
use held::ByPath;
use held::Held;
pub(crate) use held::Shares;
pub use outcome::{Forgotten, MissingEntries, Reconciled, Take, TakenBack};
pub use owed::HandbackOwed;
pub use recorded::RecordedFile;

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
    /// A rollback or a forget is taking it back: set before any removal, so a crash partway
    /// leaves an entry [`reconcile`] finishes removing, never one it takes for written (S15
    /// `PR3b` round 4).
    ///
    /// [`reconcile`]: super::apply::reconcile
    Removing,
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
    /// For a Low label entry, the explicit label the path had before ours replaced it (the
    /// lawyer's own), put back when ours goes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prior_label: Option<Label>,
    /// For a credential link's entry, the credentials file its permission entry was written on
    /// through the link: what a take-back removes it from, as the link may name another file
    /// by then (S15 `PR3b` round 4, L5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<RecordedFile>,
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
    /// matter keeps its entry; one being removed is set back to `pending`, as this launch is
    /// about to write it again, so no take-back still running and no reconcile removes it after
    /// (round 4). The plan learns each grant's entry ([`AdmittedPlan::entries`]) and which ones
    /// it added ([`AdmittedPlan::created`]). A plan already recorded is refused (`InvalidInput`).
    /// The file is untouched when every grant was listed already and none was being removed, or
    /// when the write fails.
    pub fn record_pending(&mut self, plan: &mut AdmittedPlan) -> io::Result<()> {
        if !plan.entries.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "this plan was recorded already",
            ));
        }
        let (entries, created) = self.update(true, |state| {
            let mut entries = Vec::with_capacity(plan.grants().len());
            let mut created = Vec::new();
            let mut revived = false;
            for (grant, &matter) in plan.grants().iter().zip(plan.owners()) {
                let listed = state
                    .entries()
                    .find(|entry| entry.lists(matter, grant))
                    .map(|entry| entry.id);
                if let Some(id) = listed {
                    revived |= held::revive(state, id);
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
                        prior_label: None,
                        file: None,
                    },
                    extra: Map::new(),
                }));
                entries.push(id);
                created.push(id);
            }
            let changed = revived || !created.is_empty();
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

    /// Under the lock: asks `present` about each trusted `pending` entry, with no walk; the
    /// caller walks each pending entry's tree first ([`reconcile`] does). One whose permission
    /// entry is there is marked `applied`, unless it is a deny entry: that stays `pending` for
    /// its matter's next launch to walk strictly (N4). One whose permission entry isn't is
    /// dropped, unless its matter's hand-back is owed (N5). One `present` fails on stays pending
    /// for the next start. A `removing` entry is dropped like a pending one whose permission entry
    /// is gone, and otherwise kept (only [`reconcile`] removes it), never marked `applied`.
    /// Applied entries and set aside ones are left as they are.
    pub fn reconcile(
        &mut self,
        mut present: impl FnMut(&Entry) -> io::Result<bool>,
    ) -> io::Result<Reconciled> {
        self.update(false, |state| Ok(settle::reconcile(state, &mut present)))
    }

    /// Forgets `matter`'s entries with no walk: [`Ledger::take_back`] of each of them, then the
    /// ones whose permission entry `remove` removed dropped. Entries `remove` failed on stay,
    /// `removing`, for the next try; a shared one is dropped without a removal.
    pub fn forget_matter(
        &mut self,
        matter: MatterId,
        remove: impl FnMut(&Entry) -> io::Result<()>,
    ) -> io::Result<Forgotten> {
        let taken = self.take_back(|e| (e.matter == matter).then_some(Take::Remove), remove)?;
        let ids: Vec<EntryId> = taken.removed.iter().map(|e| e.id).collect();
        self.drop_gone(&ids, |_| Ok(false))?;
        Ok(Forgotten {
            removed: taken.removed.len(),
            kept: taken.kept,
            shared: taken.shared,
        })
    }

    /// Whether another matter's entry names the same permission entry as `entry`: PR3's
    /// rollback leaves such an entry's ACE in place.
    pub fn is_shared(&self, entry: &Entry) -> bool {
        self.entries()
            .any(|other| other.matter != entry.matter && same_ace(other, entry))
    }

    /// Whether a hand-back is still owed for `pool`, as last read: a forget started and some
    /// hand-back walk hasn't succeeded yet (`owed.rs`). PR4's pool-user reuse check (R14) must
    /// refuse to reuse a pool user while this returns true. A kept hand-back this version can't
    /// read or trust counts too when it names `pool`. A hand-back the lawyer can't do (a file it
    /// can't open and can't set aside, as under the `C:\m` layout without `FILE_DELETE_CHILD`)
    /// stays owed until PR4's elevated clean-up takes ownership with admin rights, then hands it
    /// back.
    pub fn handback_owed(&self, pool: &Sid) -> bool {
        owed::owed_for(&self.state, pool)
    }

    /// Whether any entry names `sid`, in any state, as last read: a trusted entry whose SID it
    /// is, or a kept entry or kept hand-back holding it as any string value. The pool-user reuse
    /// check (S15 `PR4c`, R14) refuses a pool user while this returns true.
    pub fn names_sid(&self, sid: &Sid) -> bool {
        self.state.names_sid(sid)
    }

    /// The hand-backs still owed, as last read: each names a matter whose `forget_matter` must
    /// be run again.
    pub fn handbacks_owed(&self) -> Vec<HandbackOwed> {
        self.state.owed().cloned().collect()
    }

    /// Clears `matter`'s hand-backs owed, under the lock: once every hand-back walk for it has
    /// succeeded.
    pub(crate) fn clear_handbacks(&mut self, matter: MatterId) -> io::Result<()> {
        self.update(false, |state| Ok((owed::clear(state, matter), ())))
    }

    /// Records `label` as the explicit label entry `id`'s path had before ours: written before
    /// our label is (L4). Only an entry `plan` added, still pending, with none recorded yet, and
    /// never our own label ([`is_our_label`], N7).
    pub fn set_prior_label(
        &mut self,
        plan: &AdmittedPlan,
        id: EntryId,
        label: Label,
    ) -> io::Result<()> {
        if !plan.created.contains(&id) {
            return Ok(());
        }
        self.update(false, |state| {
            let mut changed = false;
            for slot in &mut state.slots {
                if let Slot::Known(known) = slot
                    && known.entry.id == id
                    && known.entry.state == EntryState::Pending
                    && known.entry.prior_label.is_none()
                    && !is_our_label(&label, known.entry.inherit)
                {
                    known.entry.prior_label = Some(label);
                    changed = true;
                }
            }
            Ok((changed, ()))
        })
    }

    /// Records `file` as the file entry `id`'s permission entry is written on, before it is
    /// written (round 4, L5): only an entry `plan` added, still pending, with none recorded.
    pub fn set_file(
        &mut self,
        plan: &AdmittedPlan,
        id: EntryId,
        file: &RecordedFile,
    ) -> io::Result<()> {
        if !plan.created.contains(&id) {
            return Ok(());
        }
        self.update(false, |state| Ok((recorded::set_file(state, id, file), ())))
    }

    /// The trusted `pending` entries, read again under the lock: what [`reconcile`] walks.
    ///
    /// [`reconcile`]: super::apply::reconcile
    pub fn pending(&mut self) -> io::Result<Vec<Entry>> {
        self.update(false, |state| {
            let pending = state
                .entries()
                .filter(|e| e.state == EntryState::Pending)
                .cloned()
                .collect();
            Ok((false, pending))
        })
    }

    /// Marks each of `ids` that is still `pending` `applied`: once its permission entry is
    /// known to be there and its tree walked (S15's D4, and the review's M2).
    pub fn mark_walked(&mut self, ids: &[EntryId]) -> io::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        self.update(false, |state| Ok((held::mark_walked(state, ids), ())))
    }

    /// Takes permission entries back, under one hold of the lock (the review's M3 and M4): each
    /// entry `pick` takes, read from the file now, is marked `removing` and the file
    /// written, before any removal. Then each one `pick` marks [`Take::Remove`] is handed to
    /// `remove` once per permission entry, unless an entry `pick` didn't take names the same
    /// permission entry (it is shared): a shared one, and each [`Take::Drop`], is dropped. The
    /// file is written again. Entries whose permission entry was removed stay `removing` until
    /// their trees are walked ([`Ledger::drop_gone`]); entries `remove` failed on stay
    /// `removing` for the next try. Returns the entries removed.
    pub fn take_back(
        &mut self,
        mut pick: impl FnMut(&Entry) -> Option<Take>,
        mut remove: impl FnMut(&Entry) -> io::Result<()>,
    ) -> io::Result<TakenBack> {
        self.locked(|held| held::take_back(held, None, &mut pick, &mut ByPath, &mut remove))
    }

    /// [`Ledger::take_back`], with `files` saying which names are one file (so a Low label on
    /// another name for the same file is shared, N7), and with `owe` recording the hand-backs
    /// that matter's forget owes in the same save that marks the entries `removing` (N5).
    pub(crate) fn take_back_with(
        &mut self,
        owe: Option<MatterId>,
        mut pick: impl FnMut(&Entry) -> Option<Take>,
        files: &mut dyn Shares,
        mut remove: impl FnMut(&Entry) -> io::Result<()>,
    ) -> io::Result<TakenBack> {
        self.locked(|held| held::take_back(held, owe, &mut pick, files, &mut remove))
    }

    /// Drops each of `ids` that is still `pending` or `removing` and whose permission entry
    /// `present` says
    /// is gone, under the lock: the end of a take-back, once its trees are walked. An entry
    /// whose permission entry is there again (another launch wrote it) is kept.
    pub fn drop_gone(
        &mut self,
        ids: &[EntryId],
        mut present: impl FnMut(&Entry) -> io::Result<bool>,
    ) -> io::Result<usize> {
        if ids.is_empty() {
            return Ok(0);
        }
        self.update(false, |state| {
            let gone = held::drop_gone(state, ids, &mut present);
            Ok((gone > 0, gone))
        })
    }

    /// Whether `matter`'s fence (`plan_fence` of its `folders` for `assigned` in `pool`) is
    /// listed in full as applied, as last read: the launcher's input for
    /// [`RegisteredMatter::fenced`](super::fence::RegisteredMatter::fenced).
    pub fn fence_applied(
        &self,
        matter: MatterId,
        folders: &MatterFolders,
        assigned: &Sid,
        pool: &[Sid],
    ) -> bool {
        plan_fence(folders, assigned, pool).iter().all(|grant| {
            self.entries()
                .any(|e| e.state == EntryState::Applied && e.lists(matter, grant))
        })
    }

    /// One read-modify-write under the lock (`held::update`); the cache then matches the file.
    fn update<R>(
        &mut self,
        create: bool,
        change: impl FnOnce(&mut State) -> io::Result<(bool, R)>,
    ) -> io::Result<R> {
        held::update(&self.path, &self.trust, &mut self.state, create, change)
    }

    /// Runs `change` under one hold of the lock on the file as read now (`held.rs`); the
    /// cache then holds what was last saved.
    fn locked<R>(&mut self, change: impl FnOnce(&mut Held<'_>) -> io::Result<R>) -> io::Result<R> {
        let (result, saved) = held::locked(&self.path, &self.trust, change);
        if let Some(saved) = saved {
            self.state = saved;
        }
        result
    }
}

/// Whether `label` is the Low label a [`Rights::LowLabel`] grant reaching `inherit` writes (on
/// a folder or a file): our own, never to be recorded as the lawyer's earlier label (N7).
pub(crate) fn is_our_label(label: &Label, inherit: Inherit) -> bool {
    /// `INHERITED_ACE`, which a label read may carry.
    const INHERITED: u8 = 0x10;
    /// `S-1-16-4096`'s level.
    const LOW: u32 = 4096;
    let ours: &[u8] = match inherit {
        Inherit::ThisOnly => &[0],
        // Object and container inherit on a folder; none on a file.
        Inherit::Tree => &[0, 0x3],
        // Object inherit, inherit only, no propagation.
        Inherit::FilesInFolder => &[0xd],
        // Object and container inherit, inherit only: what is below a folder.
        Inherit::Below => &[0xb],
    };
    label.level == LOW
        && label.mask == Rights::LowLabel.mask()
        && ours.contains(&(label.flags & !INHERITED))
}

/// Whether two entries name the same permission entry: path, SID, rights and inheritance.
fn same_ace(a: &Entry, b: &Entry) -> bool {
    a.path == b.path && a.sid == b.sid && a.rights == b.rights && a.inherit == b.inherit
}

#[cfg(test)]
mod tests;
