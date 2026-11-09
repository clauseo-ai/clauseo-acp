//! The launch gate (R17 PR-D2c, the orchestrator review's H1 and L2): no agent of a thread
//! launches beside a session of that thread that has ended but whose processes may still run.
//! A process keeps the jail policy it was spawned under (Seatbelt cannot take one back), and a
//! stop only sends the tree's kill (`straggler.rs`), so a read-only launch beside a live
//! read-write tree would log `ModeChanged read-only` while the old tree can still write.
//!
//! Every launch goes through `agents::start`, which asks [`gate`] first: a mode switch's and a
//! moved matter's restart, a launch after an idle stop, a crash, a sign-in stop, a failed switch
//! or a failed launch, and an unattached `SetMode`. Each ended session ([`EndedSession`]: its key,
//! its agent's tree and its Chrome) is kept on its thread ([`remember`], at `Ended` and at a
//! launch that failed after its spawn) until its processes are confirmed gone. A thread is one
//! agent in one matter, so this is per agent and matter. The check is the sandbox's
//! (`clauseo_sandbox::session_processes_gone`) and, when the session claimed browser ports, its
//! Chrome's (`straggler::wait_gone`), bounded by the run folder removal's cap
//! (`HostConfig::with_tree_wait`).
//!
//! The wait runs off the actor. Meanwhile the launch's work waits on the thread's `preparing`,
//! as a launch's checks' does, and work that comes joins it. Past the cap nothing launches: the
//! work fails with `agent.launch_failed`, so a switch logs no `ModeChanged` and the mode stays as
//! it was; the session stays kept, so the next launch waits for it again.

use std::sync::Arc;

use clauseo_process::TreeWatch;
use clauseo_protocol::{ErrorCode, ThreadId};

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::agents::{self, Waiting};
use super::super::{TurnInput, state};
use super::{Preparing, straggler};
use crate::agent::codex;
use crate::browser::session::BrowserSession;
use crate::config::SessionKey;
use crate::error::Error;

/// A thread's session whose agent has ended (or whose launch failed after its spawn), kept
/// until its processes are confirmed gone. What identifies the session to the sandbox's check:
/// today its agent's tree.
#[derive(Debug, Clone)]
pub(crate) struct EndedSession {
    /// The session's key: while a launch waits for it, it still counts as one of its matter's
    /// running sessions (`sandbox::running`).
    key: Option<SessionKey>,
    /// Its agent's tree, as `clauseo_sandbox::session_processes_gone` takes it.
    tree: TreeWatch,
    /// Its browser claims, when it had them: its Chrome leads a group of its own and runs under
    /// the session's policy too.
    browser: Option<Arc<BrowserSession>>,
}

impl EndedSession {
    /// Whether the session's processes are gone, by the non-blocking check alone: never with a
    /// Chrome, whose look is a connect, made off the actor.
    fn gone_now(&self) -> bool {
        self.browser.is_none() && clauseo_sandbox::session_processes_gone(&self.tree)
    }
}

/// The gate's wait's answer for `thread`'s launch `id`: whether its ended sessions are gone.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TreeWait {
    thread: ThreadId,
    id: u64,
    gone: bool,
}

/// Keeps `thread`'s session (`key`, its agent's `tree`, its `browser` claims) for [`gate`].
pub(in super::super) fn remember(
    actor: &mut Actor,
    thread: ThreadId,
    key: Option<SessionKey>,
    tree: TreeWatch,
    browser: Option<Arc<BrowserSession>>,
) {
    if let Some(state) = state(actor, thread) {
        state.ended.push(EndedSession { key, tree, browser });
    }
}

/// The gate every launch of `thread` passes: `waiting` back when no ended session of the thread
/// may still run. Otherwise the work waits on the thread's `preparing` while the sessions' end
/// is waited for off the actor; [`on_gone`] launches it, or fails it past the cap.
pub(in super::super) fn gate(
    actor: &mut Actor,
    thread: ThreadId,
    waiting: Vec<Waiting>,
) -> Option<Vec<Waiting>> {
    let Some(turn) = state(actor, thread) else {
        return Some(waiting);
    };
    turn.ended.retain(|session| !session.gone_now());
    if turn.ended.is_empty() {
        return Some(waiting);
    }
    let sessions = turn.ended.clone();
    let id = agents::next_agent_id();
    let key = sessions.iter().find_map(|session| session.key);
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
        return None;
    };
    let clock = Arc::clone(&actor.clock);
    let cap = actor.wiring.tree_wait;
    let held = actor.wiring.held.clone();
    let count = u32::try_from(sessions.len()).unwrap_or(u32::MAX);
    let job = codex::prepare::Job {
        what: "relaunch wait",
        matter: Some(key.matter),
        wait: (cap.saturating_mul(count)).saturating_add(codex::prepare::PREPARE_WAIT),
    };
    let Some(state) = state(actor, thread) else {
        agents::fail_all(actor, thread, waiting, &Error::Closed);
        return None;
    };
    state.preparing = Some(Box::new(Preparing { id, key, waiting }));
    tracing::info!(%thread, "the agent starts once its earlier processes are gone");
    actor.spawn_post(async move {
        let work = move || {
            let stopping = || held.stopping();
            let gone = sessions.iter().all(|session| {
                let browser = session.browser.as_deref();
                straggler::wait_gone(Some(&session.tree), browser, cap, stopping).is_ok()
            });
            Ok(gone)
        };
        let gone = codex::prepare::off_actor(clock, job, work).await;
        let gone = gone.unwrap_or(false);
        Input::Turn(TurnInput::Relaunch(TreeWait { thread, id, gone }))
    });
    None
}

/// The wait [`gate`] started ended: the thread's ended sessions are forgotten and the launch
/// goes on when they are gone; past the cap, or when the wait failed, its work fails, nothing
/// launches and they stay kept. A wait whose work was taken meanwhile (a sign-in stop) is
/// dropped. No session is added while the wait runs: no agent of the thread exists then.
pub(in super::super) fn on_gone(actor: &mut Actor, answer: TreeWait) {
    let TreeWait { thread, id, gone } = answer;
    let preparing = state(actor, thread)
        .and_then(|state| state.preparing.take_if(|preparing| preparing.id == id));
    let Some(preparing) = preparing else {
        return;
    };
    if gone {
        if let Some(state) = state(actor, thread) {
            state.ended.clear();
        }
        agents::start(actor, thread, preparing.waiting);
        return;
    }
    let cap_ms = u64::try_from(actor.wiring.tree_wait.as_millis()).unwrap_or(u64::MAX);
    tracing::warn!(
        %thread,
        cap_ms,
        "the agent's earlier processes outlived the wait; it does not start"
    );
    // A wait the host's stop cut short: the work ends as everything does at stop.
    let error = if super::super::host_stopping(actor) {
        Error::Closed
    } else {
        Error::refused(
            ErrorCode::AgentLaunchFailed,
            "The agent's earlier processes didn't stop in time, so it wasn't started again. \
             Its mode stayed as it was.",
        )
    };
    agents::fail_all(actor, thread, preparing.waiting, &error);
}
