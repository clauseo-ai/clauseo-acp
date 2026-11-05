//! The hand-backs `forget_matter` still owes (S15 `PR3b` round 2, N5). A forget records one per
//! pool user of its matter, under the lock and before any removal, and clears them only once
//! every hand-back walk has succeeded. Until then the pool user still owns what its agent made in
//! the forgotten matter's trees, so it must not be given to another matter, and the matter's
//! entries are kept: they are what a retried forget needs to find those trees.

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::Entry;
use super::file::{KnownOwed, OwedSlot, State};
use crate::windows::sids::Sid;

/// A hand-back `forget_matter` still owes: what `pool` made in `matter`'s matter, state and
/// notes folders isn't yet handed back to the desktop user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct HandbackOwed {
    /// The matter being forgotten.
    pub matter: MatterId,
    /// Its pool user.
    pub pool: Sid,
}

/// Whether a hand-back is owed for `pool` in `state`: a trusted one, or a kept one whose
/// `pool` names it.
pub(super) fn owed_for(state: &State, pool: &Sid) -> bool {
    state.owed().any(|owed| owed.pool == *pool)
        || state.handbacks.iter().any(|slot| {
            matches!(slot, OwedSlot::SetAside(value)
                if value.get("pool").and_then(Value::as_str) == Some(pool.as_str()))
        })
}

/// Clears `matter`'s trusted hand-backs owed; whether any was.
pub(super) fn clear(state: &mut State, matter: MatterId) -> bool {
    let before = state.handbacks.len();
    state
        .handbacks
        .retain(|slot| !matches!(slot, OwedSlot::Known(known) if known.owed.matter == matter));
    state.handbacks.len() != before
}

/// Records a hand-back owed for each pool user `entries` (the matter's, as read under the lock)
/// allow, unless listed already; whether any was added.
pub(super) fn owe(state: &mut State, matter: MatterId, entries: &[Entry]) -> bool {
    let mut added = false;
    for entry in entries
        .iter()
        .filter(|e| e.matter == matter && e.rights.is_allow())
    {
        let listed = state
            .owed()
            .any(|o| o.matter == matter && o.pool == entry.sid);
        if !listed {
            state.handbacks.push(OwedSlot::Known(KnownOwed {
                owed: HandbackOwed {
                    matter,
                    pool: entry.sid.clone(),
                },
                extra: Map::new(),
            }));
            added = true;
        }
    }
    added
}
