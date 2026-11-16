//! The pool's top-up: after every elevated run (a fresh set-up, an extend, a re-run) at least
//! [`MIN_FREE_AFTER_RUN`] pool users are free, so a lawyer meets the admin prompt about half as
//! often as with the 4 a pool used to start with.
//!
//! A user is free when its account exists and the reuse rule (`pool::assign`, every `Why`) would
//! hand it to a new matter once this run's resets are done: `MatterSids::held_after_resets`, the
//! one rule `assign` uses, so the count can't drift from it. A marked user no matter holds is
//! reset by this run, so it counts as free. An input that can't be read holds every user.
//!
//! [`count`] adds the shortfall on top of what the run makes anyway, through the extend's steps:
//! each new user made, logged on once, its `LocalLow` handed to Administrators and denied with the
//! rest, every registered matter fenced against it, its password saved. A fresh set-up makes 8;
//! an extend's `add` is a minimum; a re-run stays exactly a re-run (open sessions refuse it, every
//! kept password is reset, each user logged on again) and makes the shortfall too. The record
//! holds at most [`POOL_CAP`](crate::windows::pool::POOL_CAP) SIDs: at the cap the run makes what
//! fits and reports the rest ([`TopUp::short`]), which is not a failure.
//!
//! K1: the count reads the owner's `matters.json`, ledger and set-aside records, which the lawyer
//! can write. A crafted file can at worst make a run add users, up to the cap. Nothing is deleted,
//! reset or re-owned because of the count: the resets come from the marks alone (`resets.rs`).

use super::MIN_FREE_AFTER_RUN;
use super::machine::{Machine, owned};
use super::outcome::{SetupFailure, Step, TopUp, cause_of};
use crate::windows::pool::{MachineRecord, PoolUser};
use crate::windows::sids::Sid;

/// The recorded users the reuse rule holds back once this run has reset `resets` (the list it
/// acts on, so no user counts free that it won't reset), read as the owner
/// (`Machine::held_users`); none without a record. A `matters.json` that can't be read
/// fails at [`Step::Matters`], before anything changes, as
/// [`resets_due`](super::resets::resets_due) does.
pub(super) fn held(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
    resets: &[Sid],
) -> Result<Vec<Sid>, SetupFailure> {
    let Some(record) = record else {
        return Ok(Vec::new());
    };
    let mut trusted = record.pool.clone();
    trusted.push(record.group.clone());
    owned(machine, |m| m.held_users(&record.pool, &trusted, resets)).map_err(|error| {
        SetupFailure::early(
            Step::Matters,
            format!("the sandbox users in use: {}", cause_of(&error)),
        )
    })
}

/// How many users the run makes: `base` (what the set-up, re-run or extend makes anyway) and the
/// shortfall below [`MIN_FREE_AFTER_RUN`] free users, at most `room` in all. With it, what the
/// rule added and what the cap left out. `kept` are the recorded users whose accounts exist,
/// `held` those the reuse rule holds back.
pub(super) fn count(kept: &[PoolUser], held: &[Sid], base: usize, room: usize) -> (usize, TopUp) {
    let free = kept.iter().filter(|user| !held.contains(&user.sid)).count();
    let wanted = MIN_FREE_AFTER_RUN.saturating_sub(free + base);
    let make = (base + wanted).min(room);
    let added = make.saturating_sub(base.min(room));
    let top_up = TopUp {
        free_before: free,
        added,
        short: wanted - added,
    };
    if top_up.short > 0 {
        tracing::info!(
            free,
            added,
            short = top_up.short,
            "the pool is at its cap; fewer free sandbox users than wanted"
        );
    }
    (make, top_up)
}
