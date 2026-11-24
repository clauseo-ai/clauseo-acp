//! What the start-up repair of an open turn reads of the matter's logs: the cards each thread's
//! steps hold and the outside edits the matter's versions hold (`matter_worker::Logged`).

use clauseo_protocol::{MatterId, StreamId, ThreadId, ViewEntry};
use clauseo_reducer::ThreadState;

use super::super::super::Actor;
use crate::matter_worker::Logged;

/// The cards of `thread`'s steps that have an after-turn checkpoint and files.
pub(super) fn cards(thread: ThreadId, state: &ThreadState) -> Vec<Logged> {
    state
        .entries()
        .values()
        .filter_map(|entry| {
            let ViewEntry::Step(step) = entry else {
                return None;
            };
            let checkpoint = step.checkpoint_after.filter(|_| !step.files.is_empty())?;
            Some(Logged {
                checkpoint,
                thread: Some(thread),
                files: step.files.clone(),
            })
        })
        .collect()
}

/// The outside edits `matter`'s log holds, by checkpoint; none when its stream is not loaded.
pub(super) fn outside_edits(actor: &Actor, matter: MatterId) -> Vec<Logged> {
    let Some(state) = actor
        .loaded(StreamId::Matter(matter))
        .and_then(|loaded| loaded.state.as_matter())
    else {
        return Vec::new();
    };
    state
        .entries()
        .values()
        .filter_map(|entry| {
            // Only `OutsideEdits` puts files on a version.
            let ViewEntry::Version(version) = entry else {
                return None;
            };
            (!version.files.is_empty()).then(|| Logged {
                checkpoint: version.checkpoint,
                thread: None,
                files: version.files.clone(),
            })
        })
        .collect()
}
