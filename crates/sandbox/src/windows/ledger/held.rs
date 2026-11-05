//! One hold of the ledger's lock across more than one write: what a take-back needs, so its
//! entries are marked removing on disk before any permission entry is removed (S15 PR3 review,
//! M4; round 4).

use std::io;
use std::path::Path;

use clauseo_protocol::MatterId;

use super::file::{Slot, State, Trust, exists, read_state, render};
use super::{Entry, EntryId, EntryState, LEDGER_LOCK, Take, TakenBack, owed, same_ace};
use crate::windows::write;

/// The file under the lock: what was read and changed, and what was last saved.
pub(super) struct Held<'a> {
    path: Option<&'a Path>,
    pub(super) state: State,
    saved: State,
}

impl Held<'_> {
    /// Replaces the file with `state` (nothing when there is no file: no entry to change).
    pub(super) fn save(&mut self) -> io::Result<()> {
        if let Some(path) = self.path {
            render(&self.state).and_then(|bytes| write::write_atomic(path, &bytes))?;
        }
        self.saved = self.state.clone();
        Ok(())
    }
}

/// Runs `change` with the ledger at `path` read under its lock, as `Ledger::update` takes it
/// (the folder made or secured, the lock file and the ledger checked). With no ledger file,
/// `change` runs on an empty ledger and nothing is made. Returns `change`'s result and what was
/// last saved (or read), `None` when the read itself failed.
pub(super) fn locked<R>(
    path: &Path,
    trust: &Trust,
    change: impl FnOnce(&mut Held<'_>) -> io::Result<R>,
) -> (io::Result<R>, Option<State>) {
    let opened = (|| -> io::Result<Option<(std::fs::File, State)>> {
        let folder = path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "the ledger has no folder")
        })?;
        if !exists(path)? {
            return Ok(None);
        }
        write::make_sandbox_dir(folder)?;
        let lock = folder.join(LEDGER_LOCK);
        write::check_owned_file(&lock)?;
        let guard = write::lock_exclusive_for(&lock, write::LOCK_WAIT)?;
        write::check_owned_file(path)?;
        Ok(Some((guard, read_state(path, trust)?)))
    })();
    match opened {
        Err(error) => (Err(error), None),
        Ok(None) => {
            let mut held = Held {
                path: None,
                state: State::empty(),
                saved: State::empty(),
            };
            (change(&mut held), Some(State::empty()))
        }
        Ok(Some((_guard, fresh))) => {
            let mut held = Held {
                path: Some(path),
                state: fresh.clone(),
                saved: fresh,
            };
            let result = change(&mut held);
            (result, Some(held.saved))
        }
    }
}

/// One read-modify-write. With `create` false and no ledger file, there is nothing to change:
/// `change` runs on an empty ledger and nothing is made. Otherwise the folder is made or
/// secured, the lock taken, the file read again, `change` run on what was read, and the file
/// replaced if `change` says it changed something. `cache` then matches the file: the new
/// contents, or on a failed write what was read.
pub(super) fn update<R>(
    path: &Path,
    trust: &Trust,
    cache: &mut State,
    create: bool,
    change: impl FnOnce(&mut State) -> io::Result<(bool, R)>,
) -> io::Result<R> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the ledger has no folder"))?
        .to_owned();
    if !create && !exists(path)? {
        let mut state = State::empty();
        let (_, result) = change(&mut state)?;
        *cache = State::empty();
        return Ok(result);
    }
    write::make_sandbox_dir(&folder)?;
    // As `MatterSids::sid_for` does: the lock file is checked before it is locked, and the
    // ledger under the lock (owned by the user, `SYSTEM` or `Administrators`, one link, no
    // reparse point, its entries reset to the folder's).
    let lock = folder.join(LEDGER_LOCK);
    write::check_owned_file(&lock)?;
    let _held = write::lock_exclusive_for(&lock, write::LOCK_WAIT)?;
    write::check_owned_file(path)?;
    let fresh = read_state(path, trust)?;
    let mut next = fresh.clone();
    let outcome = change(&mut next);
    let (changed, result) = match outcome {
        Ok(done) => done,
        Err(error) => {
            *cache = fresh;
            return Err(error);
        }
    };
    if changed {
        if let Err(error) = render(&next).and_then(|bytes| write::write_atomic(path, &bytes)) {
            *cache = fresh;
            return Err(error);
        }
        *cache = next;
    } else {
        *cache = fresh;
    }
    Ok(result)
}

/// Sets each of `ids`'s entries to `to`.
pub(super) fn set_state(state: &mut State, ids: &[EntryId], to: EntryState) {
    for slot in &mut state.slots {
        if let Slot::Known(known) = slot
            && ids.contains(&known.entry.id)
        {
            known.entry.state = to;
        }
    }
}

/// Sets entry `id` back to `pending` if it is being removed: a launch is about to write its
/// permission entry again. Whether it was.
pub(super) fn revive(state: &mut State, id: EntryId) -> bool {
    let mut changed = false;
    for slot in &mut state.slots {
        if let Slot::Known(known) = slot
            && known.entry.id == id
            && known.entry.state == EntryState::Removing
        {
            known.entry.state = EntryState::Pending;
            changed = true;
        }
    }
    changed
}

/// How a take-back works out whether two entries name one permission entry.
pub(crate) trait Shares {
    /// Whether `a` and `b` are one file (`AclWriter::same_file`); a check that fails is
    /// `false`.
    fn same_file(&mut self, a: &Path, b: &Path) -> bool;
}

/// Paths alone: no two names are one file.
pub(crate) struct ByPath;

impl Shares for ByPath {
    fn same_file(&mut self, _a: &Path, _b: &Path) -> bool {
        false
    }
}

/// Whether `other` names `entry`'s permission entry: the same entry on the same path, the same
/// entry on one recorded credentials file (round 4, L5), or a Low label on another name for the
/// same file (the label belongs to the file, N7).
fn shares(other: &Entry, entry: &Entry, files: &mut dyn Shares) -> bool {
    let recorded = |e: &Entry| e.file.as_ref().map(|f| f.id);
    same_ace(other, entry)
        || (recorded(other).is_some()
            && recorded(other) == recorded(entry)
            && other.sid == entry.sid
            && other.rights == entry.rights
            && other.inherit == entry.inherit)
        || (other.rights.is_label()
            && entry.rights.is_label()
            && other.sid == entry.sid
            && other.path != entry.path
            && files.same_file(&other.path, &entry.path))
}

/// [`Ledger::take_back`](super::Ledger::take_back)'s work, under the lock `held` holds. With
/// `owe`, a hand-back owed is recorded for that matter's pool users in the first save, before
/// any removal (N5). A shared label entry dropped hands the label it put back, if any, to the
/// entry it shares with, so the lawyer's label still goes back when the last one goes (N7).
pub(super) fn take_back(
    held: &mut Held<'_>,
    owe: Option<MatterId>,
    pick: &mut impl FnMut(&Entry) -> Option<Take>,
    files: &mut dyn Shares,
    remove: &mut impl FnMut(&Entry) -> io::Result<()>,
) -> io::Result<TakenBack> {
    let picked: Vec<(Entry, Take)> = held
        .state
        .entries()
        .filter_map(|e| pick(e).map(|take| (e.clone(), take)))
        .collect();
    let mut done = TakenBack::default();
    if picked.is_empty() {
        return Ok(done);
    }
    let ids: Vec<EntryId> = picked.iter().map(|(e, _)| e.id).collect();
    set_state(&mut held.state, &ids, EntryState::Removing);
    if let Some(matter) = owe {
        let entries: Vec<Entry> = picked.iter().map(|(e, _)| e.clone()).collect();
        owed::owe(&mut held.state, matter, &entries);
    }
    held.save()?;
    let mut dropped: Vec<EntryId> = Vec::new();
    let mut handed: Vec<(EntryId, super::Label)> = Vec::new();
    for (entry, take) in &picked {
        let sharer = held
            .state
            .entries()
            .find(|o| !ids.contains(&o.id) && shares(o, entry, files))
            .map(|o| (o.id, o.prior_label));
        let shared = sharer.is_some();
        if let (Some((to, None)), Some(label)) = (sharer, entry.prior_label)
            && !handed.iter().any(|(id, _)| *id == to)
        {
            handed.push((to, label));
        }
        if *take == Take::Drop || shared {
            dropped.push(entry.id);
            done.shared += usize::from(shared);
            continue;
        }
        if done.removed.iter().any(|r| same_ace(r, entry)) {
            done.removed.push(entry.clone());
            continue;
        }
        match remove(entry) {
            Ok(()) => done.removed.push(entry.clone()),
            Err(error) => {
                let (kind, error) = (entry.kind, error.kind());
                tracing::warn!(%kind, %error, "an ACL entry wasn't removed");
                done.kept += 1;
            }
        }
    }
    held.state
        .slots
        .retain(|slot| !matches!(slot, Slot::Known(known) if dropped.contains(&known.entry.id)));
    for slot in &mut held.state.slots {
        if let Slot::Known(known) = slot
            && let Some((_, label)) = handed.iter().find(|(id, _)| *id == known.entry.id)
            && known.entry.prior_label.is_none()
        {
            known.entry.prior_label = Some(*label);
        }
    }
    held.save()?;
    Ok(done)
}

/// Marks each of `ids` still `pending` `applied`; whether any changed.
pub(super) fn mark_walked(state: &mut State, ids: &[EntryId]) -> bool {
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
    changed
}

/// Drops each of `ids` still `pending` or being removed whose permission entry `present` says
/// is gone; how many.
pub(super) fn drop_gone(
    state: &mut State,
    ids: &[EntryId],
    present: &mut impl FnMut(&Entry) -> io::Result<bool>,
) -> usize {
    let gone: Vec<EntryId> = state
        .entries()
        .filter(|e| {
            ids.contains(&e.id) && e.state != EntryState::Applied && matches!(present(e), Ok(false))
        })
        .map(|e| e.id)
        .collect();
    state
        .slots
        .retain(|slot| !matches!(slot, Slot::Known(known) if gone.contains(&known.entry.id)));
    gone.len()
}
