//! Switching a thread's agent (issue 4a): a lawyer's message that names another agent moves the
//! conversation to it when the message's turn starts, with a handover of the earlier
//! conversation.
//!
//! - [`target`] at `SendPrompt` keeps the agent the message names, whichever it is: the turn
//!   decides at its start whether that is a switch. When it is another agent than the thread's
//!   now, it must be in the settings (`command.invalid`), its sign-in must not refuse a launch
//!   (`sign_in::refusal_of`, with the code a launch would get), and the thread must have no
//!   background work ([`background`]: a launch preparing; held agent output waiting for a turn;
//!   or on the live agent an open permission or question, a config, mode or relay call in
//!   flight, or a command its last turn may have left running) (`thread.background_work`). All
//!   are refused before the `UserMessage` is logged. The message's `UserMessage.agent` keeps the
//!   name, so a restart restores a queued switch (`queue/restore.rs`).
//! - [`begins`] at step 2, before the launch, when the thread is not on that agent by then:
//!   background work found then sends the turn back to the held queue unchanged, and a sign-in
//!   refusal fails it. Else the live agent stops with the shutdown ladder, the thread's whole log
//!   is read and the handover built from it (`handover.rs`). Before the commit no agent of the
//!   thread may run or launch ([`quiet`]); one that does is stopped first. One commit then logs
//!   the old session's `AgentSessionDetached { Switched }`, `AgentSwitched`, the row's new agent
//!   with no session (the new agent always starts a new one), `ContextHandoff`, and the
//!   thread's picks become the message's (none yet), so the new agent's come from the matter's
//!   prefs for its kind. Once it is durable the new agent launches as any first launch does.
//! - While the switch runs (past the start, before its commit is durable) `SetConfigOption` and
//!   `SetMode` wait on it ([`ensure`]), with those the old agent had waiting, and run on the new
//!   agent after the commit; a switch that does not happen runs them on the thread's agent.
//! - No earlier finished turn: the switch goes ahead with no handover, no `ContextHandoff`.
//!   Earlier items none of which fits: the handover is its header alone, which says how many are
//!   left out. Not even the header fits: `ContextHandoff`, and the turn fails with
//!   `prompt.handoff_too_long`; no switch is logged, the thread keeps its agent.
//! - Step 5 (`prompt.rs`) sends the handover before the lawyer's words ([`prompt`]) and logs
//!   `HandoffSettled { Delivered }` once the prompt goes out, past the Codex `.codex` recheck
//!   ([`delivered`]); a turn that ends before that logs it `Failed` ([`ends`]). The lawyer's
//!   `UserMessage` stays as typed. A switch that was committed and whose turn failed before its
//!   prompt went out (a cancel during the commit included) owes the handover to the next turn.
//! - The session a switch starts gets no "context not restored" notice ([`started_by_switch`]).
//! - A `ContextHandoff` a crash left pending is settled `Failed` at start-up ([`at_start`]).

mod commit;
mod handover;
mod settle;

pub(super) use commit::committed;
pub(super) use settle::{
    at_start, delivered, ends, going_out, keeps_detach, prompt, release, started_by_switch,
};

use clauseo_protocol::{
    AgentId, DetachReason, Envelope, ErrorCode, HandoffId, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::{LogFor, TurnInput, TurnPhase, current, state, steps};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// A turn's switch to another agent, kept on its `Current`.
#[derive(Debug)]
pub(super) struct Switch {
    /// The agent the lawyer's message named.
    pub(super) to: AgentId,
    /// A handover only, from this agent: the thread switched to `to` already, and the turn
    /// that was to carry the handover failed before its prompt went out (`Kept::owed`).
    owed_from: Option<AgentId>,
    /// The agent the handover comes from, once its `ContextHandoff` is in a commit.
    from: Option<AgentId>,
    stage: Stage,
    /// The handover, once its `ContextHandoff` is in a commit.
    handoff: Option<Handoff>,
    /// The config and mode changes that came while the switch runs, for the new agent.
    held: Vec<Waiting>,
    /// The old agent's detach, logged with the switch.
    detach: Option<DetachReason>,
}

/// Where a switch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Not begun: the turn has not reached step 2.
    Asked,
    /// The old agent is stopping; its `Ended` reads the log.
    Stopping,
    /// The thread's whole log is being read.
    Reading,
    /// The switch's commit is in flight.
    Committing,
    /// Switched: the new agent launches or runs.
    Switched,
}

#[derive(Debug)]
struct Handoff {
    id: HandoffId,
    /// The text, until the prompt takes it.
    text: Option<String>,
    /// Its `HandoffSettled` is logged.
    settled: bool,
}

/// What a thread keeps of its switches beyond a turn. In memory only: after a restart a new
/// session gets the "context not restored" notice instead of an owed handover.
#[derive(Debug, Default)]
pub(super) struct Kept {
    /// The agent the thread switched from, while the handover a switch logged has not reached
    /// the new agent: the turn meant to carry it failed first, so the next one does.
    owed: Option<AgentId>,
    /// The switch's commit in flight, which may outlive its turn.
    in_flight: Option<commit::InFlight>,
}

impl Switch {
    pub(super) fn new(to: AgentId) -> Box<Self> {
        Box::new(Self {
            to,
            owed_from: None,
            from: None,
            stage: Stage::Asked,
            handoff: None,
            held: Vec::new(),
            detach: None,
        })
    }

    /// Past its start and not yet durable: config and mode changes wait on it.
    fn running(&self) -> bool {
        matches!(
            self.stage,
            Stage::Stopping | Stage::Reading | Stage::Committing
        )
    }
}

/// The agent `agent` names for a message to `thread`, kept as named. Another agent than the
/// thread's is refused with `command.invalid` when the settings do not have it, with the code
/// its launch would get when its sign-in refuses one, and with `thread.background_work` while
/// the thread may still work in the background ([`background`]).
pub(super) fn target(
    actor: &mut Actor,
    thread: ThreadId,
    agent: Option<AgentId>,
) -> Result<Option<AgentId>, Error> {
    let Some(agent) = agent else {
        return Ok(None);
    };
    if actor.check_thread(thread)?.row.agent == agent {
        return Ok(Some(agent));
    }
    if actor.effective.agent(agent.as_str()).is_none() {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("no agent {agent} in the settings"),
        ));
    }
    if let Some(refused) = super::sign_in::refusal_of(actor, &agent) {
        return Err(refused);
    }
    if background(actor, thread) {
        return Err(Error::refused(
            ErrorCode::ThreadBackgroundWork,
            "the agent may still be working in the background; send the message once it is done",
        ));
    }
    Ok(Some(agent))
}

/// Whether `thread` may still have work going that a switch would cut: a launch whose folder
/// checks run; agent output held for a turn of its own; or on its live agent an open permission
/// or question, a config, mode or relay call in flight (`idle::calls`), or a command its last
/// turn may have left running (`idle::background_may_run`). `restart_if_marked` holds a relaunch
/// for the same.
fn background(actor: &mut Actor, thread: ThreadId) -> bool {
    let (now, window) = (actor.now().mono, actor.turn_path.idle.window());
    let Some(state) = state(actor, thread) else {
        return false;
    };
    state.preparing.is_some()
        || super::own_turn::output_waits(&state.own_held)
        || state.agent.as_deref().is_some_and(|agent| {
            let open = !(agent.permissions.is_empty()
                && agent.elicitations.is_empty()
                && agent.url_elicitations.is_empty());
            open || super::idle::calls(agent) > 0
                || super::idle::background_may_run(agent, now, window)
        })
}

/// `SetConfigOption` or `SetMode` for `thread`'s agent: waits on a switch that runs, for the
/// new agent, else goes to the thread's agent.
pub(super) fn ensure(actor: &mut Actor, thread: ThreadId, waiting: Waiting) {
    if let Some(switch) = current(actor, thread)
        .and_then(|current| current.switch.as_deref_mut())
        .filter(|switch| switch.running())
    {
        tracing::info!(target: LIFECYCLE, %thread, "a config or mode change waits for the switch");
        switch.held.push(waiting);
        return;
    }
    agents::ensure(actor, thread, waiting);
}

/// Keeps `calls` on the turn's switch for the new agent; with no switch they go to the thread's.
fn hold(actor: &mut Actor, thread: ThreadId, calls: Vec<Waiting>) {
    match current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        Some(switch) => switch.held.extend(calls),
        None => release(actor, thread, calls),
    }
}

/// Step 2 of `turn`, before its launch: begins the switch its message asked for. `false` when
/// there is none (or the thread has that agent by now), and the launch goes ahead.
pub(super) fn begins(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let own = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let owes = state(actor, thread).and_then(|state| state.switch.owed.clone());
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return false;
    };
    let asked = current
        .switch
        .as_deref()
        .filter(|switch| switch.stage == Stage::Asked && own.as_ref() != Some(&switch.to))
        .map(|switch| switch.to.clone());
    let Some(to) = asked else {
        current.switch = None;
        // The thread still owes its agent the handover a failed turn did not deliver.
        return match (owes, own) {
            (Some(from), Some(to)) => {
                owed(actor, thread, turn, from, to);
                true
            }
            _ => false,
        };
    };
    if background(actor, thread) {
        tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch waits: the agent may still work; the message goes back to the held queue");
        super::queue::put_back(actor, thread, turn);
        return true;
    }
    // The sign-in may have changed since the message was taken.
    if let Some(refused) = super::sign_in::refusal_of(actor, &to) {
        tracing::info!(target: LIFECYCLE, %thread, %turn, code = refused.code().as_str(), "a switch's agent may not start");
        if let Some(current) = super::current(actor, thread) {
            current.switch = None;
        }
        steps::end_launch_failed(actor, thread, turn, refused.code(), Some(refused.detail()));
        return true;
    }
    tracing::info!(target: LIFECYCLE, %thread, %turn, "the thread switches its agent");
    let live = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if live {
        set_stage(actor, thread, Stage::Stopping);
        let calls = agents::switch_stop(actor, thread);
        hold(actor, thread, calls);
    } else {
        read_log(actor, thread, turn);
    }
    true
}

/// `turn` carries the handover its thread owes `to`, the agent it switched to from `from`,
/// since the turn that was to carry it failed before its prompt went out: the log is read and a
/// `ContextHandoff` logged as for a switch, with no `AgentSwitched` and no stop.
fn owed(actor: &mut Actor, thread: ThreadId, turn: TurnId, from: AgentId, to: AgentId) {
    tracing::info!(target: LIFECYCLE, %thread, %turn, "a turn carries the handover a failed one did not");
    if let Some(current) = current(actor, thread) {
        let mut switch = Switch::new(to);
        switch.owed_from = Some(from);
        current.switch = Some(switch);
    }
    read_log(actor, thread, turn);
}

/// `thread`'s agent ended: a switch that waited for it reads the log now, unless another agent
/// of the thread runs or launches by then ([`quiet`]).
pub(super) fn agent_ended(actor: &mut Actor, thread: ThreadId) {
    let waiting = current(actor, thread).and_then(|current| {
        let stopping = current.switch.as_deref()?.stage == Stage::Stopping;
        (stopping && current.phase == TurnPhase::Launching).then_some(current.turn)
    });
    if let Some(turn) = waiting
        && quiet(actor, thread)
    {
        read_log(actor, thread, turn);
    }
}

/// Whether no agent of `thread` runs or launches, so its switch may go on. A launch whose
/// checks run is dropped (its answer is stale then) and a live agent stopped, the switch waiting
/// for its end again; the config and mode calls waiting for either wait on the switch instead.
fn quiet(actor: &mut Actor, thread: ThreadId) -> bool {
    let Some(state) = state(actor, thread) else {
        return false;
    };
    let mut calls = state
        .preparing
        .take()
        .map(|preparing| preparing.waiting)
        .unwrap_or_default();
    let live = state.agent.is_some();
    if live {
        tracing::info!(target: LIFECYCLE, %thread, "a switch waits: an agent of the thread runs; it stops first");
        set_stage(actor, thread, Stage::Stopping);
        calls.extend(agents::switch_stop(actor, thread));
    }
    hold(actor, thread, calls);
    !live
}

fn set_stage(actor: &mut Actor, thread: ThreadId, stage: Stage) {
    if let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        switch.stage = stage;
    }
}

/// Reads `thread`'s whole log off the actor for `turn`'s handover.
fn read_log(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    set_stage(actor, thread, Stage::Reading);
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = super::super::streams::read_whole_log(&store, StreamId::Thread(thread))
            .await
            .map(|reopen| reopen.tail);
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::Switch(turn),
            result,
        })
    });
}

/// The whole log for `turn`'s handover came back: the switch's commit, or the turn's failure.
pub(super) fn on_log(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let reading = current(actor, thread).and_then(|current| {
        let switch = current.switch.as_deref()?;
        let reading = current.turn == turn
            && current.phase == TurnPhase::Launching
            && switch.stage == Stage::Reading;
        reading.then_some(switch.owed_from.is_some())
    });
    let Some(owed) = reading else {
        return;
    };
    let log = match result {
        Ok(log) => log,
        Err(error) => {
            let error = Error::Store(error);
            tracing::warn!(%thread, error = %Cause(&error), "no log for a switch's handover");
            steps::end_launch_failed(actor, thread, turn, error.code(), Some(error.detail()));
            return;
        }
    };
    // An owed handover's agent is the thread's own, and may run.
    if !owed && !quiet(actor, thread) {
        return;
    }
    if let Err(error) = commit::commit(actor, thread, turn, &log) {
        tracing::warn!(%thread, error = %Cause(&error), "a switch was not logged");
        steps::end_launch_failed(actor, thread, turn, error.code(), Some(error.detail()));
    }
}

/// `thread`'s switch as a test sees it: its turn's switch's stage and how many config and mode
/// changes it holds, and the agent the thread owes a handover from.
#[cfg(test)]
pub(super) fn seen(actor: &Actor, thread: ThreadId) -> (Option<(String, usize)>, Option<AgentId>) {
    let Some(state) = actor.threads.get(&thread).map(|entry| &entry.turn) else {
        return (None, None);
    };
    let switch = state
        .current
        .as_deref()
        .and_then(|current| current.switch.as_deref())
        .map(|switch| (format!("{:?}", switch.stage), switch.held.len()));
    (switch, state.switch.owed.clone())
}
