//! The start's check of `pending` entries without a walk, for
//! [`Ledger::reconcile`](super::Ledger::reconcile).

use std::io;

use clauseo_protocol::MatterId;

use super::file::{Slot, State};
use super::{Entry, EntryState, Reconciled};

/// [`Ledger::reconcile`](super::Ledger::reconcile)'s work on `state`, read under the lock:
/// whether it changed, and what it did.
pub(super) fn reconcile(
    state: &mut State,
    present: &mut impl FnMut(&Entry) -> io::Result<bool>,
) -> (bool, Reconciled) {
    let owed: Vec<MatterId> = state.owed().map(|o| o.matter).collect();
    let mut done = Reconciled::default();
    let mut slots = Vec::with_capacity(state.slots.len());
    for slot in std::mem::take(&mut state.slots) {
        let Slot::Known(mut known) = slot else {
            slots.push(slot);
            continue;
        };
        if known.entry.state == EntryState::Applied {
            slots.push(Slot::Known(known));
            continue;
        }
        let removing = known.entry.state == EntryState::Removing;
        match present(&known.entry) {
            // Round 4: being removed, never taken for written; only a walk-backed reconcile
            // removes it.
            Ok(true) if removing => {
                slots.push(Slot::Known(known));
                done.unchecked += 1;
            }
            // N4: a deny waits for its matter's next launch to walk it strictly.
            Ok(true) if known.entry.rights.is_deny() => {
                slots.push(Slot::Known(known));
                done.deferred += 1;
            }
            Ok(true) => {
                known.entry.state = EntryState::Applied;
                slots.push(Slot::Known(known));
                done.applied += 1;
            }
            // N5: a retried forget needs them.
            Ok(false) if owed.contains(&known.entry.matter) => {
                slots.push(Slot::Known(known));
                done.deferred += 1;
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
    (done.applied + done.dropped > 0, done)
}
