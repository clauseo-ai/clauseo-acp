//! The pool's top-up: after every elevated run (a fresh set-up, an extend, a re-run) at least
//! [`MIN_FREE_AFTER_RUN`] pool users are free, so a lawyer meets the admin prompt about half as
//! often as with the 4 a pool used to start with.
//!
//! A user is free when its account exists, no `matters.json` row holds it, no ledger entry grants
//! it access, and it isn't owed a profile reset once this run's resets are done (a marked user no
//! matter holds is reset by this run, so it counts as free). [`count`] adds the shortfall to what
//! the run makes anyway, through the same steps as an extend: each new user is made, logged on
//! once, its `LocalLow` handed to Administrators and denied with the rest, every registered
//! matter fenced against it, its password saved. An extend's `add` is a minimum. The record holds
//! at most [`POOL_CAP`](crate::windows::pool::POOL_CAP) SIDs: at the cap the run makes what fits
//! and reports the rest ([`TopUp::short`]), which is not a failure.
//!
//! A re-run that tops up goes the extend way for its kept users: no password reset and no check
//! for open sessions, so a healthy pool gets more users without resets. [`probe`] reads each kept
//! user's profile first (read-only); one missing, temporary or unreadable makes it a plain re-run
//! as before (open sessions refuse it, every kept password is reset, each user logged on again)
//! plus the shortfall, so a broken pool stays repairable, and [`TopUp::repair`] says why.
//!
//! K1: the count reads the owner's `matters.json` and ledger, which the lawyer can write. A
//! crafted file can at worst make a run add users, up to the cap. Nothing is deleted, reset or
//! re-owned because of the count: the resets come from the marks alone (`resets.rs`), as before.

use super::MIN_FREE_AFTER_RUN;
use super::machine::{Machine, owned};
use super::outcome::{SetupFailure, Step, TopUp, cause_of};
use crate::windows::pool::{MachineRecord, PoolUser};
use crate::windows::sids::Sid;

/// The recorded users a matter row holds or a ledger entry grants access to, read as the owner
/// (`Machine::held_users`); none without a record. A `matters.json` that can't be read fails at
/// [`Step::Matters`], before anything changes, as [`resets_due`](super::resets::resets_due) does.
pub(super) fn held(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
) -> Result<Vec<Sid>, SetupFailure> {
    let Some(record) = record else {
        return Ok(Vec::new());
    };
    let mut trusted = record.pool.clone();
    trusted.push(record.group.clone());
    owned(machine, |m| m.held_users(&trusted)).map_err(|error| {
        SetupFailure::early(
            Step::Matters,
            format!("the sandbox users in use: {}", cause_of(&error)),
        )
    })
}

/// How many users the run makes: `base` (what the set-up, re-run or extend makes anyway) and the
/// shortfall below [`MIN_FREE_AFTER_RUN`] free users, at most `room` in all. With it, what the
/// rule added and what the cap left out. `kept` are the recorded users whose accounts exist,
/// `held` those a matter or a grant holds.
pub(super) fn count(kept: &[PoolUser], held: &[Sid], base: usize, room: usize) -> (usize, TopUp) {
    let free = kept.iter().filter(|user| !held.contains(&user.sid)).count();
    let wanted = MIN_FREE_AFTER_RUN.saturating_sub(free + base);
    let make = (base + wanted).min(room);
    let added = make.saturating_sub(base.min(room));
    let top_up = TopUp {
        free_before: free,
        added,
        short: wanted - added,
        repair: None,
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

/// Before a re-run tops up the extend way: each kept user not owed a reset (`resets` are
/// deleted and made again anyway) must have a profile of its own. `None` when all have; else
/// the first that hasn't, and why, for a plain re-run instead.
pub(super) fn probe(
    machine: &mut impl Machine,
    kept: &[PoolUser],
    resets: &[Sid],
) -> Option<String> {
    for user in kept.iter().filter(|user| !resets.contains(&user.sid)) {
        let why = match machine.profile_of(&user.sid) {
            Ok(Some(profile)) if !profile.temporary => continue,
            Ok(Some(_)) => format!("{}'s profile is temporary", user.name),
            Ok(None) => format!("{} has no profile", user.name),
            Err(error) => format!(
                "{}'s profile can't be read: {}",
                user.name,
                cause_of(&error)
            ),
        };
        tracing::info!(%why, "a re-run that adds users resets every kept user's password");
        return Some(why);
    }
    None
}
