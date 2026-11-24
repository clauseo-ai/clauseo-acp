//! Switching a thread's agent (issue 4a): a lawyer's message that names another agent moves the
//! conversation to it when the message's turn starts, with a handover of the earlier
//! conversation.
//!
//! - [`target`] at `SendPrompt`: the named agent must be in the settings (`command.invalid`), and
//!   the thread's live agent must not have background work, as `idle::holds_relaunch` reads it
//!   (`thread.background_work`, before the `UserMessage` is logged). The thread's own agent, or
//!   none, is no switch.
//! - [`begins`] at step 2, before the launch: background work found then sends the turn back to
//!   the held queue unchanged. Else the live agent stops with the shutdown ladder (its session
//!   detaches `Switched`), the thread's whole log is read, and the handover built from it
//!   (`handover.rs`). One commit then logs `AgentSwitched`, the row's new agent with no session
//!   (the new agent always starts a new one), `ContextHandoff`, and the thread's picks become the
//!   message's (none yet), so the new agent's come from the matter's prefs for its kind. Once it
//!   is durable the new agent launches as any first launch does.
//! - A handover that does not fit logs `ContextHandoff` and fails the turn with
//!   `prompt.handoff_too_long`; the thread keeps its agent.
//! - Step 5 (`prompt.rs`) sends the handover before the lawyer's words ([`prompt`]) and logs
//!   `HandoffSettled { Delivered }` once the prompt goes out ([`delivered`]); a turn that ends
//!   before that logs it `Failed` ([`settle_failed`]). The lawyer's `UserMessage` stays as typed.
//! - The session a switch starts gets no "context not restored" notice ([`started_by_switch`]).
//!
//! Known gap: the named agent lives in memory with the queued turn, so a restart of the host
//! sends a queued message that named another agent to the thread's agent, with no switch.

mod handover;

use clauseo_protocol::{
    AgentId, AgentSwitched, ContentBlock, ContextHandoff, Envelope, ErrorCode, HandoffId,
    HandoffSettled, HandoffStatus, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::input::Input;
use super::agents::{self, Waiting};
use super::out::Out;
use super::{Current, LogFor, Then, TurnInput, TurnPhase, current, failure, state, steps};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};
use crate::pipeline::Failure;

/// The detail of `prompt.handoff_too_long`.
const TOO_LONG: &str = "The request leaves too little room for the earlier conversation. \
     Shorten the request or choose a model with a larger context; your message was not shortened.";

/// A turn's switch to another agent, kept on its `Current`.
#[derive(Debug)]
pub(super) struct Switch {
    /// The agent the lawyer's message named.
    pub(super) to: AgentId,
    /// A handover only, from this agent: the thread switched to `to` already, and the turn
    /// that was to carry the handover failed before its prompt went out ([`owed`]).
    owed_from: Option<AgentId>,
    /// The agent the handover comes from, once its `ContextHandoff` is in a commit.
    from: Option<AgentId>,
    stage: Stage,
    /// The handover, once its `ContextHandoff` is in a commit.
    handoff: Option<Handoff>,
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
    /// The row and picks before the switch's commit, put back if it fails.
    before: Option<Box<Before>>,
}

/// A thread's row and picks before a switch.
type Before = (
    clauseo_store::ThreadRow,
    Option<clauseo_protocol::ConfigPicks>,
);

impl Switch {
    pub(super) fn new(to: AgentId) -> Box<Self> {
        Box::new(Self {
            to,
            owed_from: None,
            from: None,
            stage: Stage::Asked,
            handoff: None,
        })
    }
}

/// The agent `agent` names for a message to `thread` when it is another than the thread's:
/// `None` for none or the thread's own. Refused with `command.invalid` for an agent the settings
/// do not have, and with `thread.background_work` while the thread's agent may still work.
pub(super) fn target(
    actor: &mut Actor,
    thread: ThreadId,
    agent: Option<AgentId>,
) -> Result<Option<AgentId>, Error> {
    let own = &actor.check_thread(thread)?.row.agent;
    let Some(agent) = agent.filter(|agent| agent != own) else {
        return Ok(None);
    };
    if actor.effective.agent(agent.as_str()).is_none() {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("no agent {agent} in the settings"),
        ));
    }
    if background(actor, thread) {
        return Err(Error::refused(
            ErrorCode::ThreadBackgroundWork,
            "the agent may still be working in the background; send the message once it is done",
        ));
    }
    Ok(Some(agent))
}

/// Whether `thread`'s live agent may still have work going: a config, mode or relay call in
/// flight, background work of its last turn (`idle::holds_relaunch`'s reading), or a launch
/// whose folder checks run.
fn background(actor: &mut Actor, thread: ThreadId) -> bool {
    let (now, window) = (actor.now().mono, actor.turn_path.idle.window());
    let Some(state) = state(actor, thread) else {
        return false;
    };
    state.preparing.is_some()
        || state.agent.as_deref().is_some_and(|agent| {
            super::idle::calls(agent) > 0 || super::idle::background_may_run(agent, now, window)
        })
}

/// Step 2 of `turn`, before its launch: begins the switch its message asked for. `false` when
/// there is none (or the thread has that agent by now), and the launch goes ahead.
pub(super) fn begins(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let own = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let owes = state(actor, thread).and_then(|state| state.handoff_owed.clone());
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return false;
    };
    let asked = current
        .switch
        .as_deref()
        .is_some_and(|switch| switch.stage == Stage::Asked && own.as_ref() != Some(&switch.to));
    if !asked {
        current.switch = None;
        // The thread still owes its agent the handover a failed turn did not deliver.
        return match (owes, own) {
            (Some(from), Some(to)) => {
                owed(actor, thread, turn, from, to);
                true
            }
            _ => false,
        };
    }
    if background(actor, thread) {
        tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch waits: the agent may still work; the message goes back to the held queue");
        super::queue::put_back(actor, thread, turn);
        return true;
    }
    tracing::info!(target: LIFECYCLE, %thread, %turn, "the thread switches its agent");
    let live = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if live {
        set_stage(actor, thread, Stage::Stopping);
        agents::switch_stop(actor, thread);
    } else {
        read_log(actor, thread, turn);
    }
    true
}

/// `turn` carries the handover its thread owes `to`, the agent it switched to from `from`,
/// since the turn that was to carry it failed before its prompt went out: the log is read and a
/// `ContextHandoff` logged as for a switch, with no `AgentSwitched` and no stop. In memory
/// only: after a restart the new session gets the "context not restored" notice instead.
fn owed(actor: &mut Actor, thread: ThreadId, turn: TurnId, from: AgentId, to: AgentId) {
    tracing::info!(target: LIFECYCLE, %thread, %turn, "a turn carries the handover a failed one did not");
    if let Some(current) = current(actor, thread) {
        let mut switch = Switch::new(to);
        switch.owed_from = Some(from);
        current.switch = Some(switch);
    }
    read_log(actor, thread, turn);
}

/// `thread`'s agent ended: a switch that waited for it reads the log now.
pub(super) fn agent_ended(actor: &mut Actor, thread: ThreadId) {
    let waiting = current(actor, thread).and_then(|current| {
        let stopping = current.switch.as_deref()?.stage == Stage::Stopping;
        (stopping && current.phase == TurnPhase::Launching).then_some(current.turn)
    });
    if let Some(turn) = waiting {
        read_log(actor, thread, turn);
    }
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
    let reading = current(actor, thread).is_some_and(|current| {
        current.turn == turn
            && current.phase == TurnPhase::Launching
            && current
                .switch
                .as_deref()
                .is_some_and(|switch| switch.stage == Stage::Reading)
    });
    if !reading {
        return;
    }
    let log = match result {
        Ok(log) => log,
        Err(error) => {
            let error = Error::Store(error);
            tracing::warn!(%thread, error = %Cause(&error), "no log for a switch's handover");
            steps::end_launch_failed(actor, thread, turn, error.code(), Some(error.detail()));
            return;
        }
    };
    if let Err(error) = commit(actor, thread, turn, &log) {
        tracing::warn!(%thread, error = %Cause(&error), "a switch was not logged");
        steps::end_launch_failed(actor, thread, turn, error.code(), Some(error.detail()));
    }
}

/// The switch's commit: `AgentSwitched`, the row, `ContextHandoff` and the picks; or, for a
/// handover that does not fit, `ContextHandoff` alone and the turn's failure.
fn commit(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    log: &[Envelope],
) -> Result<(), Error> {
    let entry = actor.check_thread(thread)?;
    let current = entry
        .turn
        .current
        .as_deref()
        .ok_or_else(|| super::lost("a turn"))?;
    let switch = current
        .switch
        .as_deref()
        .ok_or_else(|| super::lost("a switch"))?;
    let (to, owed_from) = (switch.to.clone(), switch.owed_from.clone());
    let from = owed_from.clone().unwrap_or_else(|| entry.row.agent.clone());
    let message = current.message;
    let built = handover::build(log, &from, &current.prompt);
    let handoff: HandoffId = actor.ids.mint(actor.now().wall)?;
    // An owed handover's agent has switched already: `from`'s options are gone, `to`'s shown.
    let owed = owed_from.is_some();
    let logged = ContextHandoff {
        handoff,
        message,
        from: handover::endpoint(actor, thread, &from, !owed),
        to: handover::endpoint(actor, thread, &to, owed),
        answers_covered: built.answers,
        items_included: built.included,
        items_left_out: built.left_out,
    };
    let mut out = Out::new(actor, thread, None)?;
    let Some(text) = built.text else {
        out.push(actor, logged)?;
        out.send(actor, Then::Nothing)?;
        keep(actor, thread, (handoff, &from), None, None);
        tracing::info!(target: LIFECYCLE, %thread, %turn, "a switch's handover does not fit");
        let code = ErrorCode::PromptHandoffTooLong;
        steps::end_launch_failed(actor, thread, turn, code, Some(TOO_LONG.to_owned()));
        return Ok(());
    };
    if owed {
        out.push(actor, logged)?;
        keep(actor, thread, (handoff, &from), Some(text), None);
        set_stage(actor, thread, Stage::Committing);
        return out.send(actor, Then::Switched(turn));
    }
    out.push(
        actor,
        AgentSwitched {
            from: from.clone(),
            to: to.clone(),
        },
    )?;
    out.push(actor, logged)?;
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| super::lost("a thread"))?;
    let before = Box::new((entry.row.clone(), entry.picks.take()));
    entry.row.agent = to;
    entry.row.acp_session_id = None;
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    keep(actor, thread, (handoff, &from), Some(text), Some(before));
    set_stage(actor, thread, Stage::Committing);
    let sent = out.send(actor, Then::Switched(turn));
    if sent.is_err() {
        put_back_row(actor, thread);
    }
    sent
}

/// Keeps `handoff` on the turn's switch, with its text and the row before the switch.
fn keep(
    actor: &mut Actor,
    thread: ThreadId,
    (id, from): (HandoffId, &AgentId),
    text: Option<String>,
    before: Option<Box<Before>>,
) {
    if let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        switch.from = Some(from.clone());
        switch.handoff = Some(Handoff {
            id,
            text,
            settled: false,
            before,
        });
    }
}

/// The switch's commit failed: the row and picks go back, and its handover was never logged.
fn put_back_row(actor: &mut Actor, thread: ThreadId) {
    let before = current(actor, thread)
        .and_then(|current| current.switch.as_deref_mut())
        .and_then(|switch| switch.handoff.take())
        .and_then(|handoff| handoff.before);
    if let Some(before) = before
        && let Some(entry) = actor.threads.get_mut(&thread)
    {
        let (row, picks) = *before;
        entry.row = row;
        entry.picks = picks;
    }
}

/// The switch's commit returned: durable, the new agent launches for `turn`; refused, the row
/// goes back and the turn ends as a turn whose commit failed.
pub(super) fn committed(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    outcome: Result<(), &Failure>,
) {
    let found = current(actor, thread).is_some_and(|current| current.turn == turn);
    match outcome {
        Ok(()) if found => {
            set_stage(actor, thread, Stage::Switched);
            // This turn carries the handover now; a failure owes it again (`settle`).
            if let Some(state) = state(actor, thread) {
                state.handoff_owed = None;
            }
            if let Some(switch) =
                current(actor, thread).and_then(|current| current.switch.as_deref_mut())
                && let Some(handoff) = &mut switch.handoff
            {
                handoff.before = None;
            }
            agents::ensure(actor, thread, Waiting::Turn(turn));
        }
        Ok(()) => {}
        Err(failed) => {
            put_back_row(actor, thread);
            if found {
                failure::on_store_failure(actor, thread, failed);
            }
        }
    }
}

/// Whether `thread`'s session starting now was started by a switch: it gets no "context not
/// restored" notice, since the handover carries the context.
pub(super) fn started_by_switch(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.current.as_deref())
        .and_then(|current| current.switch.as_deref())
        .is_some_and(|switch| switch.stage == Stage::Switched)
}

/// The prompt `current` sends: the handover first when its switch has one, else as typed.
pub(super) fn prompt(current: &mut Current) -> Vec<ContentBlock> {
    let text = current
        .switch
        .as_deref_mut()
        .and_then(|switch| switch.handoff.as_mut())
        .and_then(|handoff| handoff.text.take());
    match text {
        Some(text) => handover::wrap(&text, current.prompt.clone()),
        None => current.prompt.clone(),
    }
}

/// The prompt carrying `thread`'s handover goes out: `HandoffSettled { Delivered }`.
pub(super) fn delivered(actor: &mut Actor, thread: ThreadId) {
    settle(actor, thread, HandoffStatus::Delivered);
}

/// `thread`'s turn ends: a handover whose prompt never went out settles `Failed`.
pub(super) fn settle_failed(actor: &mut Actor, thread: ThreadId) {
    settle(actor, thread, HandoffStatus::Failed);
}

fn settle(actor: &mut Actor, thread: ThreadId, status: HandoffStatus) {
    let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut())
    else {
        return;
    };
    // A switch that was logged owes its new agent the handover until a prompt carries it.
    let switched = switch.stage == Stage::Switched;
    let from = switch.from.clone();
    let Some(handoff) = switch.handoff.as_mut().filter(|handoff| !handoff.settled) else {
        return;
    };
    handoff.settled = true;
    let settled = HandoffSettled {
        handoff: handoff.id,
        status,
    };
    if let Some(state) = state(actor, thread) {
        match status {
            HandoffStatus::Failed if switched => state.handoff_owed = from,
            HandoffStatus::Delivered => state.handoff_owed = None,
            HandoffStatus::Failed | HandoffStatus::Unknown => {}
        }
    }
    let sent = Out::new(actor, thread, None).and_then(|mut out| {
        out.push(actor, settled)?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = sent {
        tracing::warn!(%thread, error = %Cause(&error), "a handover's end was not logged");
    }
}
