//! The spawn on the actor with what a launch's checks, and the sandbox's launcher when it has
//! one ([`super::sandbox`]), found.

use clauseo_protocol::ThreadId;

use super::super::super::Actor;
use super::super::agents::{self, Waiting};
use super::super::state;
use super::{Prepared, discard, packs, sandbox};

/// The launch on the actor with what its checks (and the sandbox's launcher) found; when no agent
/// started, its run folder is removed off the actor. A spawned agent's jail (`sandbox`) rides on
/// the thread's run folder from then on.
pub(super) fn launch_prepared(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
    mut prepared: Box<Prepared>,
    sandbox: Option<sandbox::SandboxSession>,
) {
    let run = prepared.run.clone();
    let packs_skipped = std::mem::take(&mut prepared.packs_skipped);
    if let Some(state) = state(actor, thread) {
        state.run = Some(run.clone());
    }
    let failed_tree = agents::launch_now(actor, thread, waiting, *prepared);
    let launched = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if launched {
        packs::hold_notice(actor, thread, packs_skipped);
        if let Some(run) = state(actor, thread).and_then(|state| state.run.as_mut()) {
            run.sandbox = sandbox;
        }
    } else {
        if let Some(state) = state(actor, thread) {
            state.run = None;
        }
        // A process that was started and then failed is waited for like any other, by the
        // removal and by the thread's next launch.
        if let Some(tree) = failed_tree.clone() {
            let browser = run.browser_session.clone();
            super::remember(actor, thread, Some(run.key), tree, browser);
        }
        discard(actor, thread, run, failed_tree);
    }
}
