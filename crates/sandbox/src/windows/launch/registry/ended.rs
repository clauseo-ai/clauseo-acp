//! A launched session's end: the registry moves it from launched to ending once, for the
//! after-session pass (`end.rs`), whoever asks first.

use std::path::PathBuf;
use std::sync::Arc;

use super::{Entry, Registry, State};
use crate::windows::launch::prepare::Shared;
use crate::windows::launch::steps::{LaunchSteps, Slot};
use crate::windows::pool::PoolUser;

/// Where the launcher's start of a session's outer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::windows::launch) enum OuterState {
    /// Not started (or its start failed).
    Unstarted,
    /// Being started: an end meanwhile hands the slot and the matter over ([`Raced`]).
    ///
    /// [`Raced`]: OuterState::Raced
    Spawning,
    /// Started: the end waits for the pool user's processes to truly exit.
    Spawned,
    /// The end came while it was being started: the launcher frees the slot and the matter
    /// once the outer is ended and seen empty ([`finish_race`]), or leaves them stuck.
    Raced,
}

/// What `after_session` takes from a launched session's entry.
pub(in crate::windows::launch) struct Ended<S: LaunchSteps> {
    pub(in crate::windows::launch) id: u64,
    pub(in crate::windows::launch) pool: Option<PoolUser>,
    pub(in crate::windows::launch) slot: Option<Slot>,
    pub(in crate::windows::launch) run: Option<S::RunGroup>,
    /// The end's progress, for its work steps' [`Walk`](crate::windows::launch::Walk).
    pub(in crate::windows::launch) walk: Arc<Shared>,
    pub(in crate::windows::launch) spawned: bool,
    pub(in crate::windows::launch) launch_file: Option<PathBuf>,
    /// The outer was starting: the slot stays on the entry and the matter held, for the
    /// launcher to free once the outer is ended ([`finish_race`]).
    pub(in crate::windows::launch) handoff: bool,
}

/// Starts `session`'s end (of claim `id`, when given): launched, it becomes ending, with its
/// progress (the walk's stall rule and cap, not a wall bound). `None` for a session not
/// launched, or whose end (or undo) has started: each session ends once.
pub(super) fn end<S: LaunchSteps>(
    registry: &Registry<S>,
    matter: &str,
    session: &str,
    id: Option<u64>,
) -> Option<Ended<S>> {
    let now = registry.steps.now();
    let mut matters = registry.lock();
    let entry = matters.get_mut(matter).filter(|entry| {
        entry.session == session
            && entry.state == State::Launched
            && id.is_none_or(|id| entry.id == id)
    })?;
    entry.state = State::Ending;
    if entry.outer == OuterState::Spawning {
        entry.outer = OuterState::Raced;
    }
    let handoff = entry.outer == OuterState::Raced;
    let walk = Shared::new(now);
    entry.since = now;
    entry.walk = Some(Arc::clone(&walk));
    Some(Ended {
        id: entry.id,
        pool: entry.pool.clone(),
        slot: if handoff { None } else { entry.slot.clone() },
        run: entry.run.take(),
        walk,
        spawned: entry.outer == OuterState::Spawned,
        launch_file: entry.launch_file.take(),
        handoff,
    })
}

/// Frees the slot and the matter of claim `id`, whose end came while its outer was starting,
/// once the launcher has ended that outer and seen its job empty (`ended`); if it couldn't,
/// they stay held (the matter stuck): no slot or matter is freed while pool processes may live.
pub(in crate::windows::launch) fn finish_race<S: LaunchSteps>(
    registry: &Registry<S>,
    matter: &str,
    id: u64,
    ended: bool,
) {
    let ours = |entry: &&mut Entry<S>| entry.id == id && entry.outer == OuterState::Raced;
    let slot = {
        let mut matters = registry.lock();
        let Some(entry) = matters.get_mut(matter).filter(ours) else {
            return;
        };
        if !ended {
            tracing::warn!(
                "tier B couldn't end an outer its session's end raced; the matter stays"
            );
            entry.stuck_from = None;
            entry.state = State::Stuck;
            return;
        }
        entry.slot.take()
    };
    // The slot first, then the matter, as an end frees them.
    if let Some(slot) = slot {
        registry.steps.release_slot(&slot);
    }
    let mut matters = registry.lock();
    if matters
        .get(matter)
        .is_some_and(|entry| entry.id == id && entry.outer == OuterState::Raced)
    {
        matters.remove(matter);
        registry.freed.notify_all();
    }
}
