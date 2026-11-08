//! What holds a matter or a pool user past its session: a step that never returned (the matter
//! stuck until Clauseo restarts) and an `EndRun` that failed (the pool user and its slot held
//! back until a later one succeeds); and the guard that frees a matter once nothing more runs
//! for it ([`Finish`]).

use std::sync::PoisonError;
use std::time::Duration;

use super::{Entry, Registry, State};
use crate::Error;
use crate::windows::launch::prepare::{PREPARE_BOUND, PREPARE_STALL, STUCK_GRACE};
use crate::windows::launch::steps::{LaunchSteps, Slot};
use crate::windows::pool::{PoolFullCause, PoolUser};

/// Frees claim `id`'s matter when dropped: at the end of an undo or a session's end, and on a
/// panic in one of their steps too.
pub(in crate::windows::launch) struct Finish<'a, S: LaunchSteps> {
    pub(in crate::windows::launch) registry: &'a Registry<S>,
    pub(in crate::windows::launch) matter: &'a str,
    pub(in crate::windows::launch) id: u64,
}

impl<S: LaunchSteps> Drop for Finish<'_, S> {
    fn drop(&mut self) {
        self.registry.finish(self.matter, self.id);
    }
}

/// A run's group whose `EndRun` failed: tried again at each `reserve` of its pool user, which is
/// refused meanwhile; its slot stays claimed until it succeeds.
pub(in crate::windows::launch) struct Owed<S: LaunchSteps> {
    pool: Option<PoolUser>,
    run: S::RunGroup,
    slot: Option<Slot>,
}

/// Whether `entry`'s step has hung past its limit and [`STUCK_GRACE`] at `now`: a running walk
/// whose progress hasn't moved for [`PREPARE_STALL`] plus the grace (it cancels itself at the
/// stall, so its step never returned), or a reserve, an undo, a release or a session's end
/// running for [`PREPARE_BOUND`] plus the grace. Stuck once, stuck for good.
pub(super) fn stuck<S: LaunchSteps>(entry: &Entry<S>, now: Duration) -> bool {
    match (entry.state, &entry.walk) {
        (State::Stuck, _) => true,
        (State::Preparing | State::Releasing, Some(walk)) => {
            walk.idle(now) >= PREPARE_STALL.saturating_add(STUCK_GRACE)
        }
        (State::Reserving | State::Undoing | State::Releasing | State::Ending, _) => {
            now.saturating_sub(entry.since) >= PREPARE_BOUND.saturating_add(STUCK_GRACE)
        }
        (
            State::Reserved | State::Preparing | State::Prepared | State::Failed | State::Launched,
            _,
        ) => false,
    }
}

/// Ends `run` (`EndRun`), then gives `slot` back. A failed `EndRun` keeps the slot claimed and
/// the pool user `pool` held back (needing a reset) until a later `EndRun` of the group succeeds
/// (`reserve`, [`retry_owed`]). TODO(C5): the next `with_steps` (`clear_stale`) ends the groups
/// a crash left the same way, from the records.
pub(super) fn end_run_and_free_slot<S: LaunchSteps>(
    registry: &Registry<S>,
    pool: Option<&PoolUser>,
    run: Option<S::RunGroup>,
    slot: Option<Slot>,
) {
    if let Some(run) = run
        && let Err(error) = registry.steps.end_run(&run)
    {
        tracing::warn!(
            kind = error.kind(),
            "tier B's EndRun failed; the account waits"
        );
        let owed = Owed {
            pool: pool.cloned(),
            run,
            slot,
        };
        registry
            .owed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(owed);
        return;
    }
    if let Some(slot) = slot {
        registry.steps.release_slot(&slot);
    }
}

/// Tries the failed `EndRun`s of `pool` again, giving each one's slot back once it succeeds.
/// While one still fails, the pool user needs a reset: [`Error::PoolFull`], its one account
/// counted as needing a reset.
pub(super) fn retry_owed<S: LaunchSteps>(
    registry: &Registry<S>,
    pool: &PoolUser,
) -> Result<(), Error> {
    let owed_lock = || registry.owed.lock().unwrap_or_else(PoisonError::into_inner);
    let mine: Vec<Owed<S>> = {
        let mut owed = owed_lock();
        let (mine, others) = std::mem::take(&mut *owed)
            .into_iter()
            .partition(|owed| owed.pool.as_ref() == Some(pool));
        *owed = others;
        mine
    };
    let mut held = false;
    for owed in mine {
        if registry.steps.end_run(&owed.run).is_err() {
            held = true;
            owed_lock().push(owed);
        } else if let Some(slot) = owed.slot {
            registry.steps.release_slot(&slot);
        }
    }
    if held {
        return Err(Error::PoolFull {
            cause: PoolFullCause {
                size: 1,
                needs_reset: 1,
                ..PoolFullCause::default()
            },
        });
    }
    Ok(())
}
