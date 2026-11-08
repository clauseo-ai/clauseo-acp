//! The clean-ups a refused launch leaves for the elevated `--reclaim` (S15 `PR4i`): an item a
//! pool user owns that the lawyer can't hand back for want of a right
//! ([`ApplyFailure::PoolOwned`](crate::windows::writer::ApplyFailure::PoolOwned)), or a child the
//! lawyer can't open and couldn't set aside
//! ([`ApplyFailure::ChildShut`](crate::windows::writer::ApplyFailure::ChildShut)). The launch
//! records one under the lock, as the lawyer, naming the item; `--reclaim` reads the list (as
//! the owner) to pick what it walks, and never takes a path from its command line. A launch or a
//! forget of the matter that succeeds clears the matter's list.
//!
//! The lawyer also records here each item `--reclaim` reported taken ([`Stuck::Taken`], with its
//! file ID, [`owe_taken`]), so the hand-back after strips the agent's denies of the lawyer on it;
//! [`clear_taken`] drops exactly those once a hand-back refuses nothing.
//!
//! The file is the lawyer's to write, so nothing here is trusted beyond its shape: one whose
//! path isn't absolute, or that this version can't read, is kept as it is and never acted on.
//! What `--reclaim` does with a path is bounded on its own (K1): the owner, and only the owner,
//! of what a pool user owns.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use clauseo_protocol::MatterId;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::Ledger;
use super::file::State;
use crate::windows::writer::FileId;

/// Why a launch left an item for the elevated clean-up. Variants are appended from here on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stuck {
    /// A root the plan denies on that a pool user owns, whose hand-back lacked a right.
    PoolOwnedRoot,
    /// An item in a matter's tree another pool user owns, whose hand-back lacked a right.
    PoolOwnedChild,
    /// A child the lawyer can't open that couldn't be set aside.
    ChildShut,
    /// An item `--reclaim` took (its result reported it, with its file ID): the hand-back after
    /// owes it the strip of the denies an agent left of the lawyer
    /// ([`DenyStrip`](crate::windows::writer::DenyStrip)). Not a launch's refusal; cleared once a
    /// hand-back with nothing refused has run.
    Taken,
}

/// One clean-up owed: `path`, which `matter`'s launch refused for `why`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ReclaimOwed {
    /// The matter whose launch refused.
    pub matter: MatterId,
    /// The item it refused at, as the refusal named it.
    pub path: PathBuf,
    /// Why.
    pub why: Stuck,
    /// For [`Stuck::Taken`], the item's file ID, as `--reclaim` read it from the handle it took
    /// the item through: the hand-back strips denies only on that file at that path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<FileId>,
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
    Ok(ReclaimOwed {
        matter,
        path,
        why,
        file: None,
    })
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

/// Removes from `state` each trusted clean-up owed equal to one of `done`; whether any was. One
/// pass, each looked up in a set.
fn clear_these(state: &mut State, done: &[ReclaimOwed]) -> bool {
    let done: HashSet<&ReclaimOwed> = done.iter().collect();
    let before = state.reclaims.len();
    state
        .reclaims
        .retain(|slot| !matches!(slot, ReclaimSlot::Known(known) if done.contains(&known.owed)));
    state.reclaims.len() != before
}

/// Records in `ledger` under the lock, once each, the items `--reclaim` took (`matter`, final
/// path, file ID), as [`Stuck::Taken`]: what the hand-back after strips denies on. A relative
/// path is `InvalidInput` and nothing is written.
pub fn owe_taken(ledger: &mut Ledger, taken: &[(MatterId, PathBuf, FileId)]) -> io::Result<()> {
    let mut owed = Vec::with_capacity(taken.len());
    for (matter, path, file) in taken {
        let mut one = checked(*matter, path.clone(), Stuck::Taken)?;
        one.file = Some(*file);
        owed.push(one);
    }
    ledger.update(true, |state| {
        // Each looked up in a set of what is listed: one pass however many there are.
        let mut listed_now: HashSet<ReclaimOwed> = listed(state).cloned().collect();
        let mut added = false;
        for one in owed {
            if listed_now.insert(one.clone()) {
                let extra = Map::new();
                state
                    .reclaims
                    .push(ReclaimSlot::Known(KnownReclaim { owed: one, extra }));
                added = true;
            }
        }
        Ok((added, ()))
    })
}

/// Clears from `ledger` under the lock exactly the clean-ups owed in `done` (the hand-back's
/// [`Stuck::Taken`] items once it refused nothing); with none of them listed, touches nothing.
pub fn clear_taken(ledger: &mut Ledger, done: &[ReclaimOwed]) -> io::Result<()> {
    let wanted: HashSet<&ReclaimOwed> = done.iter().collect();
    if !listed(&ledger.state).any(|owed| wanted.contains(owed)) {
        return Ok(());
    }
    ledger.update(false, |state| Ok((clear_these(state, done), ())))
}
