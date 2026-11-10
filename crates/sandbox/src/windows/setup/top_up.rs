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
use crate::windows::pool::assign::Why;
use crate::windows::pool::{MachineRecord, POOL_CAP, PoolUser};
use crate::windows::sids::Sid;

/// A pool user the reuse rule holds back, and why: `None` when an input couldn't be read,
/// which holds every user.
pub type HeldUser = (Sid, Option<Why>);

/// The recorded users the reuse rule holds back once this run has reset `resets` (the list it
/// acts on, so no user counts free that it won't reset), read as the owner
/// (`Machine::held_users`); none without a record. A `matters.json` that can't be read
/// fails at [`Step::Matters`], before anything changes, as
/// [`resets_due`](super::resets::resets_due) does.
pub(super) fn held(
    machine: &mut impl Machine,
    record: Option<&MachineRecord>,
    resets: &[Sid],
) -> Result<Vec<HeldUser>, SetupFailure> {
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
/// shortfall below [`MIN_FREE_AFTER_RUN`] free users, at most `room` in all: the record never
/// holds more than [`POOL_CAP`] SIDs (D-B5), the pool's hard ceiling, free and held alike, so
/// held users can never push it past. At the ceiling the top-up adds what fits and reports the
/// rest ([`TopUp::short`]), never failing the run. With it, what the rule added and what the
/// cap left out. `kept` are the recorded users whose accounts exist, `held` those the reuse
/// rule holds back. Every run logs its [`Line`].
pub(super) fn count(
    kept: &[PoolUser],
    held: &[HeldUser],
    base: usize,
    room: usize,
) -> (usize, TopUp) {
    let free = kept
        .iter()
        .filter(|user| !held.iter().any(|(sid, _)| *sid == user.sid))
        .count();
    let wanted = MIN_FREE_AFTER_RUN.saturating_sub(free + base);
    let make = (base + wanted).min(room);
    let added = make.saturating_sub(base.min(room));
    let top_up = TopUp {
        free_before: free,
        added,
        short: wanted - added,
    };
    let line = Line::of(&top_up, kept, held);
    tracing::info!(
        free = line.free,
        held_by_matters = line.matters,
        held_by_set_aside = line.set_aside,
        held_for_reset = line.reset,
        held_unread = line.unread,
        added = line.added,
        short = line.short,
        ceiling = line.ceiling,
        "the pool's top-up"
    );
    (make, top_up)
}

/// What the top-up's log line says: the free users, the held ones by why, how many the run
/// added, how many the ceiling left out, and the ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Line {
    /// Recorded users the reuse rule would hand to a new matter.
    pub(super) free: usize,
    /// Held by a matter: its row, its open session, a ledger grant or a hand-back owed.
    pub(super) matters: usize,
    /// Owning an item set aside (or a set-aside manifest that can't be read).
    pub(super) set_aside: usize,
    /// Marked for a reset this run doesn't do.
    pub(super) reset: usize,
    /// Held because an input couldn't be read.
    pub(super) unread: usize,
    /// Users the top-up added.
    pub(super) added: usize,
    /// Users the ceiling left out.
    pub(super) short: usize,
    /// The ceiling: [`POOL_CAP`].
    pub(super) ceiling: u8,
}

impl Line {
    /// The line for `top_up`, over the kept users and those `held` holds back.
    pub(super) fn of(top_up: &TopUp, kept: &[PoolUser], held: &[HeldUser]) -> Line {
        let mut line = Line {
            free: top_up.free_before,
            matters: 0,
            set_aside: 0,
            reset: 0,
            unread: 0,
            added: top_up.added,
            short: top_up.short,
            ceiling: POOL_CAP,
        };
        let kept_held = held
            .iter()
            .filter(|(sid, _)| kept.iter().any(|user| user.sid == *sid));
        for (_, why) in kept_held {
            match why {
                Some(Why::Held(_) | Why::Open | Why::Owed | Why::Entries) => line.matters += 1,
                Some(Why::SetAside) => line.set_aside += 1,
                Some(Why::NeedsReset) => line.reset += 1,
                None => line.unread += 1,
            }
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(rid: u32) -> PoolUser {
        PoolUser {
            name: format!("ClauseoSb{:02}", rid - 1000),
            sid: Sid::parse(&format!("S-1-5-21-5-5-5-{rid}")).unwrap(),
        }
    }

    #[test]
    fn the_line_counts_the_free_and_each_reason_held() {
        // The VM row: eight users, ClauseoSb01 held by set-aside records, so one is added.
        let kept: Vec<PoolUser> = (1001..=1008).map(user).collect();
        let held = vec![(kept[0].sid.clone(), Some(Why::SetAside))];
        let (make, top_up) = count(&kept, &held, 0, 24);
        assert_eq!(make, 1);
        let line = Line::of(&top_up, &kept, &held);
        let expected = Line {
            free: 7,
            matters: 0,
            set_aside: 1,
            reset: 0,
            unread: 0,
            added: 1,
            short: 0,
            ceiling: POOL_CAP,
        };
        assert_eq!(line, expected);
        let held = vec![
            (kept[0].sid.clone(), Some(Why::Entries)),
            (kept[1].sid.clone(), Some(Why::Open)),
            (kept[2].sid.clone(), Some(Why::NeedsReset)),
            (kept[3].sid.clone(), None),
            // Not a kept user (a dead SID): not counted.
            (user(1020).sid, Some(Why::SetAside)),
        ];
        let (_, top_up) = count(&kept, &held, 0, 24);
        let line = Line::of(&top_up, &kept, &held);
        assert_eq!(
            (
                line.free,
                line.matters,
                line.set_aside,
                line.reset,
                line.unread
            ),
            (4, 2, 0, 1, 1)
        );
        assert_eq!((line.added, line.short), (4, 0));
    }

    #[test]
    fn held_users_never_push_the_pool_past_its_ceiling() {
        // 30 recorded, every one held: room for two, the rest reported short.
        let kept: Vec<PoolUser> = (1001..=1030).map(user).collect();
        let held: Vec<HeldUser> = kept.iter().map(|u| (u.sid.clone(), None)).collect();
        let room = usize::from(POOL_CAP) - kept.len();
        let (make, top_up) = count(&kept, &held, 0, room);
        assert_eq!((make, top_up.added, top_up.short), (2, 2, 6));
        // At the ceiling: nothing made, no failure, the shortfall reported.
        let kept: Vec<PoolUser> = (1001..=1032).map(user).collect();
        let held: Vec<HeldUser> = kept.iter().map(|u| (u.sid.clone(), None)).collect();
        let (make, top_up) = count(&kept, &held, 0, 0);
        assert_eq!((make, top_up.added, top_up.short), (0, 0, 8));
    }
}
