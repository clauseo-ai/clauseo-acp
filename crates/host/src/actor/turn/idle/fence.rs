//! A change to the fence between matters reaching a live agent. An agent's fence is made once,
//! at its launch (`agent::matter_fence`), so an agent whose launch would fence other matters
//! now is stopped as an idle agent is: its session detaches `IdleStopped`, which the app shows
//! as resting, and its next prompt relaunches it with the new rules and resumes the session.
//!
//! - After each input, each live agent's launch key (`FenceKey`: the matters' ids and folders,
//!   the conversation's invites) is compared with the key a launch would take now
//!   (`turn::fence`). A different key stops it once [`super::stoppable`]: nothing pending,
//!   so a turn, a queued prompt, a permission or a launch keeps it until it ends (a busy agent
//!   keeps its old rules until then; the host's auto-deny reads the new ones at once). A change
//!   put back before then stops nothing. A stopping agent is not stoppable, so quick changes
//!   make one stop, and the agent relaunched takes the newest key.
//! - Shape shared with the Tools change (branch tools-reach-open, `idle/tools.rs`): once that
//!   lands, this becomes one reason of its stale mark, with one stop path.
//! - The log names the thread only.

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::agents;
use super::super::fence::{key_of, matters_hash};
use crate::log_file::LIFECYCLE;

/// Stops each live agent whose fence is out of date and that has nothing pending.
pub(super) fn stop_stale(actor: &mut Actor) {
    let live: Vec<ThreadId> = actor.turn_path.idle.live.iter().copied().collect();
    if live.is_empty() {
        return;
    }
    let matters = matters_hash(actor);
    for thread in live {
        let launched = actor
            .threads
            .get(&thread)
            .and_then(|entry| entry.turn.run.as_ref())
            .map(|run| run.data_deny.matters.key);
        let Some(launched) = launched else {
            continue;
        };
        if launched != key_of(actor, thread, matters)
            && super::stoppable(actor, thread) == Some(true)
        {
            tracing::info!(target: LIFECYCLE, %thread, "agent stopped for a matter fence change");
            agents::idle_stop(actor, thread);
        }
    }
}
