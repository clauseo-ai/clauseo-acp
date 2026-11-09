//! The wait before a restarted agent's successor launches (a jailed Codex's mode switch,
//! `agents::restart_for_switch`; a moved matter, `agents::restart_if_moved`): only once the
//! stopped agent's tree is gone ([`straggler::wait_gone`], with the run folder removal's cap,
//! `HostConfig::with_tree_wait`). A process keeps the jail policy it was spawned under (Seatbelt
//! cannot take one back), and the tree's kill is only sent (`straggler.rs`), so a read-only
//! relaunch beside a live read-write tree would log `ModeChanged read-only` while the old tree
//! can still write.
//!
//! The wait runs off the actor. Meanwhile the successor's work waits on the thread's
//! `preparing`, as a launch's checks' does, and work that comes joins it. Past the cap nothing
//! launches: the work fails with `agent.launch_failed`, so a switch logs no `ModeChanged` and
//! the mode stays as it was.

use clauseo_process::TreeWatch;
use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::agents::{self, Waiting};
use super::super::{TurnInput, state};
use super::{Preparing, straggler};
use crate::agent::codex;
use crate::config::SessionKey;
use crate::error::Error;

/// The wait's answer for `thread`'s relaunch `id`: whether the old tree is gone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TreeWait {
    thread: ThreadId,
    id: u64,
    gone: bool,
}

/// Launches `thread`'s agent for `waiting` once the stopped agent's tree (`tree`) is gone.
/// `key` is the stopped session's: while the wait runs it still counts as one of its matter's
/// running sessions (`sandbox::running`).
pub(in super::super) fn after_tree(
    actor: &mut Actor,
    thread: ThreadId,
    key: Option<SessionKey>,
    tree: TreeWatch,
    waiting: Vec<Waiting>,
) {
    if clauseo_sandbox::session_processes_gone(&tree) {
        agents::start(actor, thread, waiting);
        return;
    }
    let id = agents::next_agent_id();
    let key = key.or_else(|| {
        let matter = actor.check_thread(thread).ok()?.row.matter;
        Some(SessionKey {
            matter,
            thread,
            launch: id,
        })
    });
    let Some(key) = key else {
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    };
    let clock = std::sync::Arc::clone(&actor.clock);
    let cap = actor.wiring.tree_wait;
    let held = actor.wiring.held.clone();
    let job = codex::prepare::Job {
        what: "relaunch wait",
        matter: Some(key.matter),
        wait: cap.saturating_add(codex::prepare::PREPARE_WAIT),
    };
    let Some(state) = state(actor, thread) else {
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return;
    };
    state.preparing = Some(Box::new(Preparing { id, key, waiting }));
    tracing::info!(%thread, "the agent starts again once its old processes are gone");
    actor.spawn_post(async move {
        let work = move || {
            let stopping = || held.stopping();
            Ok(straggler::wait_gone(Some(&tree), None, cap, stopping).is_ok())
        };
        let gone = codex::prepare::off_actor(clock, job, work).await;
        let gone = gone.unwrap_or(false);
        Input::Turn(TurnInput::Relaunch(TreeWait { thread, id, gone }))
    });
}

/// The wait [`after_tree`] started ended: the agent launches when the old tree is gone; past
/// the cap, or when the wait failed, its work fails and nothing launches. A wait whose work was
/// taken meanwhile (a sign-in stop) is dropped.
pub(in super::super) fn on_gone(actor: &mut Actor, answer: TreeWait) {
    let TreeWait { thread, id, gone } = answer;
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        return;
    };
    if gone {
        agents::start(actor, thread, preparing.waiting);
        return;
    }
    let cap_ms = u64::try_from(actor.wiring.tree_wait.as_millis()).unwrap_or(u64::MAX);
    tracing::warn!(
        %thread,
        cap_ms,
        "the stopped agent's processes outlived the wait; it does not start again"
    );
    // A wait the host's stop cut short: the work ends as everything does at stop.
    let error = if super::super::host_stopping(actor) {
        Error::Closed
    } else {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            "the agent's earlier processes did not exit in time",
        )
    };
    agents::fail_all(actor, thread, preparing.waiting, &error);
}
