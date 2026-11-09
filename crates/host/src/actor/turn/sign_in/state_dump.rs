//! The turn path's state for a host test whose wait timed out (`HostHandle::state_for_test`):
//! each thread's agent, launch, turn and queue, the `stop_agents` calls waiting and the
//! listings running, as ids, stages and counts. No path and no prompt text: gate logs are
//! pasted around.

use std::fmt::Write as _;

use super::super::super::Actor;
use crate::agent::leftovers;

/// One line per thread, then the stop waits, the listings and the host clock.
pub(crate) fn state_for_test(actor: &mut Actor) -> String {
    let mut out = String::new();
    for (thread, entry) in &actor.threads {
        let state = &entry.turn;
        let agent = state.agent.as_deref().map_or_else(
            || "none".to_owned(),
            |agent| {
                let (step, calls) = (agent.link.owner_step(), agent.link.fenced_calls());
                format!(
                    "#{} stage={:?} session={} closed={} owner={step:?} fenced_calls={calls} \
                     waiting={} after_stop={}",
                    agent.id,
                    agent.stage,
                    agent.session.is_some(),
                    agent.link.closed(),
                    agent.waiting.len(),
                    agent.after_stop.len(),
                )
            },
        );
        let current = state.current.as_deref().map_or_else(
            || "none".to_owned(),
            |current| {
                format!(
                    "{} phase={:?} cancelled={} prompt_call={} cancel_timer={}",
                    current.turn,
                    current.phase,
                    current.cancelled,
                    current.prompt_call.is_some(),
                    current.cancel_timer.is_some(),
                )
            },
        );
        let _infallible = writeln!(
            out,
            "thread {thread} agent_id={}: agent {agent}; preparing={}; turn {current}; \
             closing={}; stopped_for_sign_in={}; queue {}",
            entry.row.agent,
            state.preparing.is_some(),
            state.closing.is_some(),
            state.stopped_for_sign_in.is_some(),
            super::super::queue::dump::describe_for_test(&state.queue),
        );
    }
    let held: Vec<String> = actor
        .wiring
        .sign_in
        .held
        .iter()
        .map(|(agent, why)| format!("{agent}:{why:?}"))
        .collect();
    let _infallible = writeln!(
        out,
        "stop waits {}; held [{}]; listings running {}; host clock mono {:?}",
        actor.turn_path.stops.describe_for_test(),
        held.join(", "),
        leftovers::listings_running(),
        actor.now().mono,
    );
    out
}
