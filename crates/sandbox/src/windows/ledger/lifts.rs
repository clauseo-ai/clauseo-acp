//! A fence's deny on a matter folder alone, lifted for a session (R18): a session granted a read
//! of another matter's whole folder takes that matter's deny of its own pool user off the
//! folder itself, so it can list it, and puts it back when the grant goes
//! (`apply/reads.rs`). The entry stays the fenced matter's; the lift is marked on it
//! ([`Entry::lifted`]), under the lock and before the deny is removed, and cleared only once
//! the deny is back. A crash in between leaves the mark, so the session's matter's next launch,
//! its forget, or [`reconcile`](crate::windows::apply::reconcile) puts the deny back. While a
//! mark names the pool user, the reuse check holds it back ([`Ledger::grants_sid`]).

use std::io;

use clauseo_protocol::MatterId;

use super::file::Slot;
use super::{Entry, EntryId, EntryState, Ledger};
use crate::windows::acl_plan::{Inherit, Rights};
use crate::windows::admit::PathKind;

/// Marks entry `id` lifted by `by`, or clears its mark (`None`), under the lock. Only a fence's
/// deny on a matter folder alone, not being removed, takes a mark; any entry loses one. Whether
/// the file changed.
pub(crate) fn set_lifted(
    ledger: &mut Ledger,
    id: EntryId,
    by: Option<MatterId>,
) -> io::Result<bool> {
    ledger.update(false, |state| {
        let mut changed = false;
        for slot in &mut state.slots {
            if let Slot::Known(known) = slot
                && known.entry.id == id
                && known.entry.lifted != by
                && (by.is_none() || liftable(&known.entry))
            {
                known.entry.lifted = by;
                changed = true;
            }
        }
        Ok((changed, changed))
    })
}

/// Whether `entry` is a fence's deny on a matter folder alone, not being removed.
pub(crate) fn liftable(entry: &Entry) -> bool {
    entry.kind == PathKind::Matter
        && entry.rights == Rights::DenyReadWrite
        && entry.inherit == Inherit::ThisOnly
        && entry.state != EntryState::Removing
}
