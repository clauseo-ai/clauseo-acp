//! A change to the fence between matters reaching a live agent. An agent's fence is made once,
//! at its launch (`agent::matter_fence`), so an agent whose launch would fence other matters
//! now is stopped as an idle agent is: its session detaches `IdleStopped`, which the app shows
//! as resting, and its next prompt relaunches it with the new rules and resumes the session.
//!
//! - After each input, each live agent's launch key (`FenceKey`: the matters' ids and folders,
//!   the conversation's invites) is compared with the key a launch would take now
//!   (`turn::outside::fence`). A different key stops it once [`super::stoppable`]: nothing pending,
//!   so a turn, a queued prompt, a permission or a launch keeps it until it ends (a busy agent
//!   keeps its old rules until then; the host's auto-deny reads the new ones at once). A turn
//!   that ends with prompts queued stops a stale agent before the next one starts
//!   ([`stop_stale_at_turn_end`]): the relaunch sends the queue. A change
//!   put back before then stops nothing. A stopping agent is not stoppable, so quick changes
//!   make one stop, and the agent relaunched takes the newest key.
//! - Shape shared with the Tools change (branch tools-reach-open, `idle/tools.rs`): once that
//!   lands, this becomes one reason of its stale mark, with one stop path.
//! - The log names the thread only.

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::agents;
use super::super::outside::fence::{key_of, matters_hash};
use crate::log_file::LIFECYCLE;

/// Stops each live agent whose fence is out of date and that has nothing pending.
pub(super) fn stop_stale(actor: &mut Actor) {
    let live: Vec<ThreadId> = actor.turn_path.idle.live.iter().copied().collect();
    if live.is_empty() {
        return;
    }
    let matters = matters_hash(actor);
    for thread in live {
        if stale(actor, thread, matters) && super::stoppable(actor, thread) == Some(true) {
            tracing::info!(target: LIFECYCLE, %thread, "agent stopped for a matter fence change");
            agents::idle_stop(actor, thread);
        }
    }
}

/// A turn of `thread` closed and its queue holds more: an agent whose fence is out of date is
/// stopped now, before the queue moves on, so a queued prompt (one sent after a withdrawn
/// invite) runs on the relaunched agent with the new rules. The head waits for the stop
/// (`agents::after_stop`), and the relaunch takes it. Called before `queue::move_on`.
pub(in crate::actor::turn) fn stop_stale_at_turn_end(actor: &mut Actor, thread: ThreadId) {
    let waits = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| !entry.turn.queue.is_empty());
    let attached = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.agent.as_deref())
        .is_some_and(|agent| agent.stage == super::super::agents::Stage::Attached);
    if waits && attached && stale(actor, thread, matters_hash(actor)) {
        tracing::info!(
            target: LIFECYCLE,
            %thread,
            "agent stopped at its turn's end for a matter fence change"
        );
        agents::idle_stop(actor, thread);
    }
}

/// Whether `thread`'s live launch took another fence key than a launch would take now, for a
/// prompt about to reach it (`agents::ensure`): a read invite withdrawn or a matter added while
/// the prompt waited on its command stops the agent first, and the prompt waits for the
/// relaunch, so it never runs on the old rules.
pub(in crate::actor::turn) fn fence_stale(actor: &Actor, thread: ThreadId) -> bool {
    stale(actor, thread, matters_hash(actor))
}

/// Whether `thread`'s live launch took another fence key than a launch would take now.
fn stale(actor: &Actor, thread: ThreadId, matters: u64) -> bool {
    let launched = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.run.as_ref())
        .map(|run| run.data_deny.matters.key);
    launched.is_some_and(|launched| launched != key_of(actor, thread, matters))
}
