//! One hold of the ledger's lock across more than one write: what a take-back needs, so its
//! entries are `pending` on disk before any permission entry is removed (S15 PR3 review, M4).

use std::io;
use std::path::Path;

use super::file::{Slot, State, Trust, exists, read_state, render};
use super::{Entry, EntryId, EntryState, LEDGER_LOCK, Take, TakenBack, same_ace};
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

/// [`Ledger::take_back`](super::Ledger::take_back)'s work, under the lock `held` holds.
pub(super) fn take_back(
    held: &mut Held<'_>,
    pick: &mut impl FnMut(&Entry) -> Option<Take>,
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
    set_state(&mut held.state, &ids, EntryState::Pending);
    held.save()?;
    let mut dropped: Vec<EntryId> = Vec::new();
    for (entry, take) in &picked {
        let shared = held
            .state
            .entries()
            .any(|o| !ids.contains(&o.id) && same_ace(o, entry));
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

/// Drops each of `ids` still `pending` whose permission entry `present` says is gone; how many.
pub(super) fn drop_gone(
    state: &mut State,
    ids: &[EntryId],
    present: &mut impl FnMut(&Entry) -> io::Result<bool>,
) -> usize {
    let gone: Vec<EntryId> = state
        .entries()
        .filter(|e| {
            ids.contains(&e.id) && e.state == EntryState::Pending && matches!(present(e), Ok(false))
        })
        .map(|e| e.id)
        .collect();
    state
        .slots
        .retain(|slot| !matches!(slot, Slot::Known(known) if gone.contains(&known.entry.id)));
    gone.len()
}
