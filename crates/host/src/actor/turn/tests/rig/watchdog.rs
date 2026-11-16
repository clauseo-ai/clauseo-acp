//! The failure of a wait for an input that ran out (`Rig::next`): the step waited for, then
//! what a hang leaves to read, as ids, stages and counts (no path, no prompt text): the work in
//! flight, the inputs held back, the host clock's timers, whether each agent's process and
//! its group still run, and each thread's agent with its owner task's step (`state_for_test`).
//! Every bound on an agent's stop runs on the frozen `ManualClock`, so a real event that never
//! comes (an exit, a pipe's end, a listing) hangs; the owner's step names the one that did.

use super::super::super::super::HostPhase;
use super::super::super::sign_in::state_for_test;
use super::{Rig, WATCHDOG};

/// The panic message for a wait for `what` that ran out.
pub(super) fn report(rig: &mut Rig, what: &str) -> String {
    let agents = state_for_test(&mut rig.actor);
    let phase = match rig.actor.phase {
        HostPhase::Starting(_) => "starting",
        HostPhase::Serving => "serving",
        HostPhase::Stopping(_) => "stopping",
    };
    // Whether each agent's process still runs and whether its group (its tree) is empty: an
    // agent that exited with its group not empty left a process that may hold its pipes.
    let processes: Vec<String> = rig
        .actor
        .threads
        .values()
        .filter_map(|entry| entry.turn.agent.as_deref())
        .map(|agent| {
            let pid = agent.link.pid();
            let running = clauseo_process::running(pid);
            let tree_gone = agent.link.tree_watch().gone();
            format!("#{} running={running} tree_gone={tree_gone}", agent.id)
        })
        .collect();
    let held: Vec<String> = rig.held.iter().map(|input| format!("{input:?}")).collect();
    format!(
        "no input for {WATCHDOG:?} waiting for {what}; host {phase}; work {:?}; held [{}]; \
         host clock timers armed {}; agent processes [{}]\n{agents}",
        rig.actor.work,
        held.join(", "),
        rig.clock.armed(),
        processes.join(", "),
    )
}
