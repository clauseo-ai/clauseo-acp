//! The relay's case-law results on the thread (R24, PR6): what `sources/case_law.rs` joins to
//! the agent's own tool call when it completes.

use clauseo_protocol::ThreadId;

use super::super::Actor;
use super::state;

/// The relay found what a case-law call of `thread` returned: kept for the sources join
/// (`sources/case_law.rs`) until the agent's tool call completes, or five minutes pass. A
/// thread that is not loaded drops it: its call cannot complete in a live turn.
pub(crate) fn found(actor: &mut Actor, thread: ThreadId, found: crate::connectors::CaseLawFound) {
    let now = actor.now().mono;
    if let Some(state) = state(actor, thread) {
        state.sources.case_law.found(now, found);
    } else {
        tracing::debug!(%thread, "a case-law result for a thread not loaded was dropped");
    }
}
