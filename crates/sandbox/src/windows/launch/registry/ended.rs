//! A launched session's end: the registry moves it from launched to ending once, for the
//! after-session pass (`end.rs`), whoever asks first.

use std::path::PathBuf;
use std::sync::Arc;

use super::{Registry, State};
use crate::windows::launch::prepare::Shared;
use crate::windows::launch::steps::{LaunchSteps, Slot};
use crate::windows::pool::PoolUser;

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
    let walk = Shared::new(now);
    entry.since = now;
    entry.walk = Some(Arc::clone(&walk));
    Some(Ended {
        id: entry.id,
        pool: entry.pool.clone(),
        slot: entry.slot.clone(),
        run: entry.run.take(),
        walk,
        spawned: entry.spawned,
        launch_file: entry.launch_file.take(),
    })
}
