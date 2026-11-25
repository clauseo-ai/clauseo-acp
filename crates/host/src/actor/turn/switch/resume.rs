//! Going back to an agent the conversation had before (issue 4b), as T3 Code does
//! (`orchestration-v2/Orchestrator.ts`): the agent takes back its own last session of this
//! conversation, and the handover holds only what it has not seen.
//!
//! - [`last_session`]: the session the agent last had in this thread, read from this thread's
//!   log alone: each `AgentSessionAttached` belongs to the agent the thread had then
//!   (`ThreadCreated`'s, then each `AgentSwitched`'s new one). Another thread's sessions are
//!   never read, so they can never be loaded here. None when the agent never ran here.
//! - [`seen_until`]: the last turn that session saw: the latest ended turn in which the agent,
//!   in that session, answered (any text, content or tool call of its own) or had a handover
//!   delivered (`HandoffSettled { Delivered }` of a `ContextHandoff` to it). A turn the agent
//!   never saw (a launch that failed, a prompt refused) leaves no such mark, so it is handed
//!   over again; a mark on a turn that never ended counts for nothing. Handing over too much
//!   repeats what the session holds; too little would lose it, so every doubt hands over more.
//! - The switch's row names that session, so the launch loads (or resumes) it as any restart
//!   does (`session.rs`). Whether it loads is known only when the session attaches: so a switch
//!   back to an agent whose session saw part of the conversation logs its `ContextHandoff` in
//!   the attach's commit ([`attached`]), with only what came after `seen_until` and `resumed`
//!   when the session loaded, else the whole conversation (a new session: the agent could no
//!   longer load its own). Until then the two handovers wait on the switch ([`Pending`]).
//! - Such a handover is logged only with its session's attach: the attach commit carries it
//!   (`Then::Attached`), and a refused one takes it back ([`unlogged`]), so no
//!   `HandoffSettled` ever names a handover the log does not hold; it is owed instead.
//! - It lives in memory until then, so a stop loses it: start-up owes it again ([`at_start`])
//!   when the thread's last handover went to another agent than the thread's, and the owed
//!   turn's log read confirms it ([`since_switch`]): the thread's last switch to its agent has
//!   no handover delivered since. An attach since proves nothing (a Stop during the load, a
//!   restart or a launch for a model change attaches with no turn to take the handover), so
//!   [`seen_until`] decides: the owed turn loads the session the row names, and hands over only
//!   what it missed when it loads (nothing when it saw everything), else all.

use clauseo_protocol::{
    AgentId, AgentSessionId, ContextHandoff, DomainEvent, Endpoint, Envelope, HandoffId,
    HandoffStatus, MessageId, StreamId, ThreadId, TurnId, ViewEntry,
};
use std::collections::{BTreeMap, BTreeSet};

use super::super::super::Actor;
use super::super::out::Out;
use super::super::{current, lost, state};
use super::handover::{self, Built, Fit};
use super::{Handoff, Stage};
use crate::error::Error;
use crate::log_file::LIFECYCLE;

/// The session `agent` last had in this thread, from the thread's own `log`.
pub(super) fn last_session(log: &[Envelope], agent: &AgentId) -> Option<AgentSessionId> {
    let mut now: Option<&AgentId> = None;
    let mut last = None;
    for envelope in log {
        if let DomainEvent::ThreadCreated(created) = &envelope.event {
            now = Some(&created.agent);
        } else if let DomainEvent::AgentSwitched(switched) = &envelope.event {
            now = Some(&switched.to);
        } else if let DomainEvent::AgentSessionAttached(attached) = &envelope.event
            && now == Some(agent)
        {
            last = Some(attached.session.clone());
        }
    }
    last
}

/// The last turn `agent` saw in `session`, from the thread's own `log`: the latest ended turn
/// in which it answered in that session, or had a handover delivered there. None when it saw
/// none: the handover then covers the whole conversation.
pub(super) fn seen_until(
    log: &[Envelope],
    agent: &AgentId,
    session: &AgentSessionId,
) -> Option<TurnId> {
    let mut now: Option<&AgentId> = None;
    let mut attached: Option<&AgentSessionId> = None;
    // The handovers to `agent`, by the lawyer's message they went with, and each message's turn.
    let mut handoffs: BTreeMap<HandoffId, MessageId> = BTreeMap::new();
    let mut turns: BTreeMap<MessageId, TurnId> = BTreeMap::new();
    let mut seen: BTreeSet<TurnId> = BTreeSet::new();
    for envelope in log {
        let ours = now == Some(agent) && attached == Some(session);
        let event = &envelope.event;
        if let DomainEvent::ThreadCreated(created) = event {
            now = Some(&created.agent);
        } else if let DomainEvent::AgentSwitched(switched) = event {
            now = Some(&switched.to);
            attached = None;
        } else if let DomainEvent::AgentSessionAttached(event) = event {
            attached = Some(&event.session);
        } else if let DomainEvent::AgentSessionDetached(_) = event {
            attached = None;
        } else if let DomainEvent::UserMessage(message) = event {
            turns.insert(message.message, message.turn);
        } else if let DomainEvent::ContextHandoff(handoff) = event
            && handoff.to.agent == *agent
        {
            handoffs.insert(handoff.handoff, handoff.message);
        } else if let DomainEvent::HandoffSettled(settled) = event
            && ours
            && settled.status == HandoffStatus::Delivered
        {
            let turn = handoffs
                .get(&settled.handoff)
                .and_then(|message| turns.get(message));
            seen.extend(turn);
        } else if ours && let Some(turn) = own_output(event) {
            seen.insert(turn);
        }
    }
    log.iter().rev().find_map(|envelope| {
        if let DomainEvent::TurnEnded(ended) = &envelope.event
            && seen.contains(&ended.turn)
        {
            Some(ended.turn)
        } else {
            None
        }
    })
}

/// The turn of the agent's own output: text, content or a tool call.
fn own_output(event: &DomainEvent) -> Option<TurnId> {
    if let DomainEvent::TextAppended(text) = event {
        Some(text.turn)
    } else if let DomainEvent::ContentAppended(content) = event {
        Some(content.turn)
    } else if let DomainEvent::ToolCallStarted(call) = event {
        Some(call.turn)
    } else {
        None
    }
}

/// The two ends of a handover, with the lawyer's message it goes with.
pub(super) type Ends = (MessageId, Endpoint, Endpoint);

/// What a switch's commit hands over (`commit.rs`).
pub(super) struct Plan {
    /// The agent it comes from: for an owed handover, the one the thread last switched from.
    pub(super) from: AgentId,
    /// The handover logged with the commit; `Fit::Nothing` when it waits (`pending`).
    pub(super) built: Built,
    /// It holds only what the agent's own session had not seen.
    pub(super) resumed: bool,
    pub(super) ends: Ends,
    /// The session the target takes back: the row names it for its launch.
    pub(super) session: Option<AgentSessionId>,
    /// The handovers waiting for that session's attach.
    pub(super) pending: Option<Box<Pending>>,
}

/// The handover of `turn`'s switch of `thread` from `from` to `to` (or the one it owes `to`),
/// from the thread's whole `log`. The session `to` takes back is its last one in this thread,
/// or for an owed handover the one the thread's row names (the switch's, or the one attached
/// since). When that session saw part of the conversation and it is not running yet, whether
/// it loads is known only once it attaches: both handovers wait for that, unless there is
/// nothing to hand over or not even the header fits. Running already (an owed handover), it
/// took the session back: only what it had not seen, decided now.
pub(super) fn plan(
    actor: &mut Actor,
    (thread, turn): (ThreadId, TurnId),
    log: &[Envelope],
    (from, to, owed): (&AgentId, &AgentId, bool),
) -> Result<Plan, Error> {
    let since = owed.then(|| since_switch(log, to));
    let from = since
        .as_ref()
        .and_then(|since| since.from.as_ref())
        .unwrap_or(from);
    let entry = actor.check_thread(thread)?;
    let current = entry
        .turn
        .current
        .as_deref()
        .ok_or_else(|| lost("a turn"))?;
    let message = current.message;
    let session = if owed {
        entry.row.acp_session_id.clone()
    } else {
        last_session(log, to)
    };
    let after = session
        .as_ref()
        .and_then(|session| seen_until(log, to, session));
    let built = handover::build(log, from, &current.prompt, None);
    let delta = after.map(|after| handover::build(log, from, &current.prompt, Some(after)));
    let live = owed && session.is_some() && live_session(actor, thread) == session;
    // Owed in error: the agent has had it, or the thread never switched to it (`at_start`).
    let none_owed = since.as_ref().is_some_and(|since| !since.owes());
    let ends = (
        message,
        handover::endpoint(actor, thread, from, !owed),
        if owed {
            handover::endpoint(actor, thread, to, true)
        } else {
            // The new agent has no session yet: what the turn's picks and the agent's kept
            // options say it starts with (`prompt_picks.rs`).
            let (model, effort) =
                super::super::prompt_picks::starting_with(actor, thread, turn, to.as_str());
            Endpoint {
                agent: to.clone(),
                model,
                effort,
            }
        },
    );
    let (built, resumed, pending) = match (delta, session.clone()) {
        _ if none_owed => (Built::nothing(), false, None),
        (Some(delta), _) if live => (delta, true, None),
        (Some(delta), Some(session)) if matches!(built.fit, Fit::Text(_)) => {
            let pending = Pending {
                session,
                message,
                from: ends.1.clone(),
                to: ends.2.clone(),
                delta,
                full: built,
            };
            (Built::nothing(), false, Some(Box::new(pending)))
        }
        (_, _) => (built, false, None),
    };
    Ok(Plan {
        from: from.clone(),
        built,
        resumed,
        ends,
        session,
        pending,
    })
}

/// A switch's two handovers, waiting for the target's session to attach: only what came after
/// the session last saw the conversation when it loads, the whole conversation when it does
/// not.
#[derive(Debug)]
pub(super) struct Pending {
    /// The session the launch loads.
    pub(super) session: AgentSessionId,
    pub(super) message: MessageId,
    pub(super) from: Endpoint,
    pub(super) to: Endpoint,
    /// What came after the session last saw the conversation; `Fit::Nothing` when nothing did.
    pub(super) delta: Built,
    /// The whole conversation; never `Fit::Nothing` or `Fit::TooLong` (`commit.rs` logs those
    /// as a switch with no session to load does).
    pub(super) full: Built,
}

/// The `ContextHandoff` of `built`.
pub(super) fn event(
    handoff: HandoffId,
    (message, from, to): Ends,
    built: &Built,
    resumed: bool,
) -> ContextHandoff {
    ContextHandoff {
        handoff,
        message,
        from,
        to,
        answers_covered: built.answers,
        items_included: built.included,
        items_left_out: built.left_out,
        resumed,
    }
}

/// `thread`'s session `session` attaches in a commit `out` builds (`session.rs`), `loaded`
/// when it was loaded or resumed: the handover its switch waits with goes in that commit, only
/// what came after the session last saw the conversation when it is the one the switch asked
/// for and it loaded, else the whole conversation. Nothing to hand over: no `ContextHandoff`,
/// and an owed handover is owed no more. The handover logged, if any: the attach commit is sent
/// with `Then::Attached` of it, and a refused one takes it back ([`unlogged`]).
pub(in super::super) fn attached(
    actor: &mut Actor,
    thread: ThreadId,
    (session, loaded): (&AgentSessionId, bool),
    out: &mut Out,
) -> Result<Option<HandoffId>, Error> {
    let Some(pending) = current(actor, thread)
        .and_then(|current| current.switch.as_deref_mut())
        .filter(|switch| switch.stage == Stage::Switched)
        .and_then(|switch| switch.pending.take())
    else {
        return Ok(None);
    };
    let Pending {
        session: asked,
        message,
        from,
        to,
        delta,
        full,
    } = *pending;
    let resumed = loaded && *session == asked;
    tracing::info!(target: LIFECYCLE, %thread, resumed, "a switch's agent attached; its handover goes with it");
    let mut built = if resumed { delta } else { full };
    let Fit::Text(text) = std::mem::replace(&mut built.fit, Fit::Nothing) else {
        // The session saw everything the conversation holds.
        if let Some(state) = state(actor, thread) {
            state.switch.owed = None;
        }
        return Ok(None);
    };
    let pushed = actor
        .ids
        .mint(actor.now().wall)
        .and_then(|handoff: HandoffId| {
            out.push(actor, event(handoff, (message, from, to), &built, resumed))?;
            Ok(handoff)
        });
    let handoff = match pushed {
        Ok(handoff) => handoff,
        Err(error) => {
            owe(actor, thread);
            return Err(error);
        }
    };
    if let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut()) {
        switch.handoff = Some(Handoff {
            id: handoff,
            text: Some(text),
            settled: false,
            went_out: super::settle::WentOut::default(),
        });
    }
    Ok(Some(handoff))
}

/// The attach commit holding `handoff` was refused, or never sent: the handover is not in the
/// log, so it is never settled; while the switch's turn runs, the next turn owes it instead.
/// (A turn that ended first settled it in a commit after the refused one, which the store
/// refused too, and owed it unless its prompt went out.)
pub(in super::super) fn unlogged(actor: &mut Actor, thread: ThreadId, handoff: HandoffId) {
    let Some(switch) = current(actor, thread).and_then(|current| current.switch.as_deref_mut())
    else {
        return;
    };
    if switch
        .handoff
        .as_ref()
        .is_none_or(|kept| kept.id != handoff)
    {
        return;
    }
    tracing::info!(target: LIFECYCLE, %thread, "a switch's handover was not logged with its attach; the next turn owes it");
    switch.handoff = None;
    owe(actor, thread);
}

/// The switch of `thread`'s turn owes its handover to the next turn.
fn owe(actor: &mut Actor, thread: ThreadId) {
    let from = current(actor, thread)
        .and_then(|current| current.switch.as_deref())
        .filter(|switch| switch.stage == Stage::Switched)
        .and_then(|switch| switch.from.clone());
    if let (Some(from), Some(state)) = (from, state(actor, thread)) {
        state.switch.owed = Some(from);
    }
}

/// Start-up, after 4a's `settle::at_start`: nothing owed yet, and the thread's last handover
/// went to another agent than the thread's, so a switch since may have lost the handover it
/// held for its session's attach. Owed from that handover's agent, the one most likely
/// switched from; the owed turn's log read confirms or drops it, and names the agent
/// ([`since_switch`], [`plan`]).
pub(in super::super) fn at_start(actor: &mut Actor, thread: ThreadId) {
    let Some(agent) = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone())
    else {
        return;
    };
    let last = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| {
            state
                .entries()
                .values()
                .filter_map(|entry| {
                    if let ViewEntry::Handoff(handoff) = entry {
                        Some(handoff)
                    } else {
                        None
                    }
                })
                .max_by_key(|handoff| handoff.order)
                .map(|handoff| handoff.to.agent.clone())
        });
    let Some(to) = last.filter(|to| *to != agent) else {
        return;
    };
    if let Some(state) = state(actor, thread)
        && state.switch.owed.is_none()
    {
        tracing::info!(target: LIFECYCLE, %thread, "the thread moved on from its last handover's agent; its next turn checks for a handover a stop lost");
        state.switch.owed = Some(to);
    }
}

/// What came after the thread's last switch to `to`, from the thread's whole `log`.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Since {
    /// The agent it switched from; none when the log has no switch to `to`.
    pub(super) from: Option<AgentId>,
    /// A `ContextHandoff` to `to` logged since was delivered, or went out unconfirmed.
    pub(super) delivered: bool,
}

impl Since {
    /// Whether `to` is still owed a handover: the thread switched to it and none was
    /// delivered since. A session attached since does not settle it: what that session saw
    /// is [`seen_until`]'s to say, so the delta comes out empty only when it saw everything.
    pub(super) fn owes(&self) -> bool {
        self.from.is_some() && !self.delivered
    }
}

/// What came after the thread's last switch to `to` in `log`.
pub(super) fn since_switch(log: &[Envelope], to: &AgentId) -> Since {
    let mut since = Since::default();
    let mut handoffs: BTreeSet<HandoffId> = BTreeSet::new();
    for envelope in log {
        let event = &envelope.event;
        if let DomainEvent::AgentSwitched(switched) = event
            && switched.to == *to
        {
            since = Since {
                from: Some(switched.from.clone()),
                ..Since::default()
            };
            handoffs.clear();
        } else if let DomainEvent::ContextHandoff(handoff) = event
            && handoff.to.agent == *to
        {
            handoffs.insert(handoff.handoff);
        } else if let DomainEvent::HandoffSettled(settled) = event
            && matches!(
                settled.status,
                HandoffStatus::Delivered | HandoffStatus::Unconfirmed
            )
            && handoffs.contains(&settled.handoff)
        {
            since.delivered = true;
        }
    }
    since
}

/// The session `thread`'s agent runs now, attached: an owed handover to it is decided now.
pub(super) fn live_session(actor: &mut Actor, thread: ThreadId) -> Option<AgentSessionId> {
    state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .and_then(|agent| agent.session.clone())
}

/// The flag of how far the prompt carrying `thread`'s handover got, for a test.
#[cfg(test)]
pub(in super::super) fn handoff_went(
    actor: &mut Actor,
    thread: ThreadId,
) -> Option<super::settle::WentOut> {
    current(actor, thread)
        .and_then(|current| current.switch.as_deref())
        .and_then(|switch| switch.handoff.as_ref())
        .map(|handoff| std::rc::Rc::clone(&handoff.went_out))
}

#[cfg(test)]
#[path = "resume/tests.rs"]
mod tests;
