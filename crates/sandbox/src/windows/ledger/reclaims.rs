//! The clean-ups a refused launch leaves for the elevated `--reclaim` (S15 `PR4i`): an item a
//! pool user owns that the lawyer can't hand back for want of a right
//! ([`ApplyFailure::PoolOwned`](crate::windows::writer::ApplyFailure::PoolOwned)), or a child the
//! lawyer can't open and couldn't set aside
//! ([`ApplyFailure::ChildShut`](crate::windows::writer::ApplyFailure::ChildShut)). The launch
//! records one under the lock, as the lawyer, naming the item; `--reclaim` reads the list (as
//! the owner) to pick what it walks, and never takes a path from its command line. A launch or a
//! forget of the matter that succeeds clears the matter's list.
//!
//! The file is the lawyer's to write, so nothing here is trusted beyond its shape: one whose
//! path isn't absolute, or that this version can't read, is kept as it is and never acted on.
//! What `--reclaim` does with a path is bounded on its own (K1): the owner, and only the owner,
//! of what a pool user owns.

use std::io;
use std::path::PathBuf;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::file::State;

/// Why a launch left an item for the elevated clean-up. Variants are appended from here on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stuck {
    /// A root the plan denies on that a pool user owns, whose hand-back lacked a right.
    PoolOwnedRoot,
    /// An item in a matter's tree another pool user owns, whose hand-back lacked a right.
    PoolOwnedChild,
    /// A child the lawyer can't open that couldn't be set aside.
    ChildShut,
}

/// One clean-up owed: `path`, which `matter`'s launch refused for `why`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReclaimOwed {
    /// The matter whose launch refused.
    pub matter: MatterId,
    /// The item it refused at, as the refusal named it.
    pub path: PathBuf,
    /// Why.
    pub why: Stuck,
}

/// A clean-up owed as read: one this version reads and trusts, with the fields it doesn't know,
/// or one kept as it was.
#[derive(Debug, Clone)]
pub(super) enum ReclaimSlot {
    Known(KnownReclaim),
    SetAside(Value),
}

/// A clean-up owed this version reads, and the fields it doesn't know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct KnownReclaim {
    #[serde(flatten)]
    pub(super) owed: ReclaimOwed,
    #[serde(flatten)]
    pub(super) extra: Map<String, Value>,
}

/// One clean-up owed as read: known when this version reads it and its path is absolute.
pub(super) fn read_reclaim(value: Value) -> ReclaimSlot {
    match KnownReclaim::deserialize(&value) {
        Ok(known) if known.owed.path.is_absolute() => ReclaimSlot::Known(known),
        Ok(_) | Err(_) => ReclaimSlot::SetAside(value),
    }
}

/// The list's JSON, as written back: a kept one unchanged.
pub(super) fn render_reclaims(slots: &[ReclaimSlot]) -> serde_json::Result<Vec<Value>> {
    slots
        .iter()
        .map(|slot| match slot {
            ReclaimSlot::Known(known) => serde_json::to_value(known),
            ReclaimSlot::SetAside(value) => Ok(value.clone()),
        })
        .collect()
}

/// `matter`, `path` and `why` as a clean-up owed; a relative `path` is `InvalidInput`.
pub(super) fn checked(matter: MatterId, path: PathBuf, why: Stuck) -> io::Result<ReclaimOwed> {
    if !path.is_absolute() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a clean-up's path must be absolute",
        ));
    }
    Ok(ReclaimOwed { matter, path, why })
}

/// The clean-ups owed in `state` this version reads and trusts.
pub(super) fn listed(state: &State) -> impl Iterator<Item = &ReclaimOwed> {
    state.reclaims.iter().filter_map(|slot| match slot {
        ReclaimSlot::Known(known) => Some(&known.owed),
        ReclaimSlot::SetAside(_) => None,
    })
}

/// Adds `owed` to `state` unless it is listed already; whether it was added.
pub(super) fn owe(state: &mut State, owed: ReclaimOwed) -> bool {
    if listed(state).any(|listed| *listed == owed) {
        return false;
    }
    let extra = Map::new();
    state
        .reclaims
        .push(ReclaimSlot::Known(KnownReclaim { owed, extra }));
    true
}

/// Clears `matter`'s trusted clean-ups owed from `state`; whether any was.
pub(super) fn clear(state: &mut State, matter: MatterId) -> bool {
    let before = state.reclaims.len();
    state
        .reclaims
        .retain(|slot| !matches!(slot, ReclaimSlot::Known(known) if known.owed.matter == matter));
    state.reclaims.len() != before
}
