//! What holds a matter or a pool user past its session: a step that never returned (the matter
//! stuck until Clauseo restarts) and an `EndRun` that failed (the pool user and its slot held
//! back until a later one succeeds); and the guard that frees a matter once nothing more runs
//! for it ([`Finish`]).

use std::sync::PoisonError;
use std::time::Duration;

use super::{Entry, Registry, State};
use crate::Error;
use crate::windows::launch::prepare::{PREPARE_BOUND, STUCK_GRACE};
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
        finish(self.registry, self.matter, self.id);
    }
}

/// Frees claim `id`'s matter at the end of `after_session`, even if a step panics, and even
/// if a step hung long enough to make it stuck ([`finish_end`]).
pub(in crate::windows::launch) struct FinishEnd<'a, S: LaunchSteps> {
    pub(in crate::windows::launch) registry: &'a Registry<S>,
    pub(in crate::windows::launch) matter: &'a str,
    pub(in crate::windows::launch) id: u64,
}

impl<S: LaunchSteps> Drop for FinishEnd<'_, S> {
    fn drop(&mut self) {
        finish_end(self.registry, self.matter, self.id);
    }
}

/// Frees `matter` once claim `id`'s session's end has returned: ending, or stuck in its
/// end (its steps returned at last, so nothing more runs for it). Stuck for another reason
/// (a launcher that couldn't end its outer), it stays held.
fn finish_end<S: LaunchSteps>(registry: &Registry<S>, matter: &str, id: u64) {
    let mut matters = registry.lock();
    let ours = |entry: &Entry<S>| {
        entry.id == id
            && (entry.state == State::Ending
                || (entry.state == State::Stuck && entry.stuck_from == Some(State::Ending)))
    };
    if let Some(entry) = matters.get(matter).filter(|entry| ours(entry)) {
        if entry.state == State::Stuck {
            tracing::info!("tier B's stuck session end returned; the matter is free");
        }
        matters.remove(matter);
        registry.freed.notify_all();
    }
}

/// Frees `matter` once claim `id`'s session has ended; a stuck one stays held.
fn finish<S: LaunchSteps>(registry: &Registry<S>, matter: &str, id: u64) {
    let mut matters = registry.lock();
    let ours = |entry: &Entry<S>| entry.id == id && entry.state != State::Stuck;
    if matters.get(matter).is_some_and(ours) {
        matters.remove(matter);
        registry.freed.notify_all();
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
/// or session's end whose progress hasn't moved for [`PREPARE_STALL`] plus the grace, or that
/// ran past [`WORK_CEILING`] plus it (it stops itself at either, so its step never returned:
/// [`Shared::hung`]); or a reserve, an undo or a release running for [`PREPARE_BOUND`] plus the
/// grace. Stuck once, stuck until restart, but for a session's end whose steps return
/// ([`finish_end`]).
///
/// [`PREPARE_STALL`]: crate::windows::launch::PREPARE_STALL
/// [`WORK_CEILING`]: crate::windows::launch::WORK_CEILING
/// [`Shared::hung`]: crate::windows::launch::prepare::Shared::hung
pub(super) fn stuck<S: LaunchSteps>(entry: &Entry<S>, now: Duration) -> bool {
    match (entry.state, &entry.walk) {
        (State::Stuck, _) => true,
        (State::Preparing | State::Releasing | State::Ending, Some(walk)) => walk.hung(now),
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
/// (`reserve`, [`retry_owed`]). TODO(C5, launch-blocking in 0049): what is owed lives in this
/// process only; C5 persists it, or `clear_stale` finds run groups still populated at start and
/// ends them before any reuse.
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
