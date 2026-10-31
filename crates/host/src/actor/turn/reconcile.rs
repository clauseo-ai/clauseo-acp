//! Start-up reconciliation: a thread whose log ends inside a turn (the host stopped without
//! ending it) gets one commit. Pending requests expire `HostRestarted`; a started turn gets an
//! after-turn checkpoint and ends `TurnFailed { thread.interrupted }`; a turn that never
//! started stays queued, held behind its own thread; an attached session is detached. A turn
//! with a `TurnEnded` is never touched, so each repair happens once.

use std::collections::BTreeSet;

use clauseo_matters::{Checkpoint, TurnMeta};
use clauseo_protocol::{
    AgentSessionDetached, DetachReason, DomainEvent, ElicitationId, ElicitationState, Envelope,
    ErrorCode, MatterPath, PermissionRequestId, PermissionState, StopReason, StreamId, ThreadId,
    ThreadStatus, ToolCallId, TurnId, TurnQueued, UserMessageStatus, ViewEntry,
};
use clauseo_reducer::ThreadState;

use super::super::Actor;
use super::super::input::{Input, MatterTag};
use super::close::{Expiries, TurnClose};
use super::out::Out;
use super::{JobKind, LogFor, Then, TurnInput, TurnJob, end, matter_of, state, updates};
use crate::error::Error;
use crate::matter_worker::MatterJob;
use crate::sent::SentView;

/// A started turn's repair, while its after-turn checkpoint runs.
#[derive(Debug)]
pub(crate) struct Repair {
    turn: TurnId,
    pending: Pending,
}

/// What the repair commit closes besides the turn.
#[derive(Debug, Clone, Default)]
struct Pending {
    permissions: Vec<PermissionRequestId>,
    elicitations: Vec<ElicitationId>,
    /// A session was attached and never detached.
    attached: bool,
}

/// Looks at the loaded thread; one with an open turn or a pending request has its whole log
/// read (a snapshot keeps neither its session nor its queue events).
pub(super) fn reconcile(actor: &mut Actor, thread: ThreadId) {
    let stream = StreamId::Thread(thread);
    let needs = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| open_turn(state).is_some() || has_pending(state));
    if !needs {
        return;
    }
    if let Some(state) = state(actor, thread) {
        state.reconciling = true;
    }
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = super::super::streams::read_whole_log(&store, stream)
            .await
            .map(|reopen| reopen.tail);
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::Reconcile,
            result,
        })
    });
}

/// The open turn: the last `UserMessage` not removed and without a step, and whether it
/// started (the thread is `Running` or `NeedsYou`).
fn open_turn(state: &ThreadState) -> Option<(TurnId, bool)> {
    let mut stepped: BTreeSet<TurnId> = BTreeSet::new();
    let mut last = None;
    for entry in state.entries().values() {
        if let ViewEntry::Step(step) = entry {
            stepped.insert(step.turn);
        } else if let ViewEntry::UserMessage(message) = entry
            && !matches!(message.status, UserMessageStatus::Removed)
        {
            last = Some(message.turn);
        }
    }
    let turn = last.filter(|turn| !stepped.contains(turn))?;
    let started = matches!(
        state.status(),
        ThreadStatus::Running | ThreadStatus::NeedsYou
    );
    Some((turn, started))
}

fn has_pending(state: &ThreadState) -> bool {
    state.entries().values().any(|entry| {
        if let ViewEntry::Permission(permission) = entry {
            matches!(permission.state, PermissionState::Pending)
        } else if let ViewEntry::Elicitation(elicitation) = entry {
            matches!(elicitation.state, ElicitationState::Pending)
        } else {
            false
        }
    })
}

/// The whole log came back: repair now, or after the after-turn checkpoint.
pub(super) fn on_log(
    actor: &mut Actor,
    thread: ThreadId,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let log = match result {
        Ok(log) => log,
        Err(error) => {
            tracing::warn!(%thread, %error, "a thread could not be reconciled");
            done(actor, thread);
            return;
        }
    };
    let stream = StreamId::Thread(thread);
    let view = SentView::rebuild(stream, &log);
    let pending = Pending {
        permissions: view.permissions().map(|(id, _)| id).collect(),
        elicitations: view.elicitations().map(|(id, _)| id).collect(),
        attached: view.session().is_some(),
    };
    let open = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(open_turn);
    match open {
        Some((turn, true)) => after_turn(actor, thread, turn, &log, pending),
        Some((turn, false)) => {
            let queued = needs_queued(&log, thread, turn)
                .then(|| TurnQueued::new(thread, turn, thread, previous_turn(&log, turn)));
            let logged = (|| -> Result<(), Error> {
                let mut out = Out::new(actor, thread, None)?;
                pending.expiries().push_into(actor, &mut out)?;
                if let Some(queued) = queued {
                    out.push(
                        actor,
                        queued.map_err(|error| Error::Internal(error.to_string()))?,
                    )?;
                }
                detach(actor, &mut out, &pending)?;
                out.send(actor, Then::Nothing)
            })();
            if let Err(error) = logged {
                tracing::warn!(%thread, %error, "a queued turn was not held");
            }
            done(actor, thread);
        }
        None => {
            let logged = (|| -> Result<(), Error> {
                let mut out = Out::new(actor, thread, None)?;
                pending.expiries().push_into(actor, &mut out)?;
                detach(actor, &mut out, &pending)?;
                out.send(actor, Then::Nothing)
            })();
            if let Err(error) = logged {
                tracing::warn!(%thread, %error, "pending requests were not expired");
            }
            done(actor, thread);
        }
    }
}

/// Whether the turn's last `TurnQueued` (if any) waits for another thread or is missing.
fn needs_queued(log: &[Envelope], thread: ThreadId, turn: TurnId) -> bool {
    let last = log.iter().rev().find_map(|envelope| {
        if let DomainEvent::TurnQueued(queued) = &envelope.event
            && queued.turn() == turn
        {
            Some(queued.behind())
        } else {
            None
        }
    });
    last != Some(thread)
}

/// The thread's turn before `turn`, by its `UserMessage`s.
fn previous_turn(log: &[Envelope], turn: TurnId) -> Option<TurnId> {
    let mut previous = None;
    for envelope in log {
        if let DomainEvent::UserMessage(message) = &envelope.event {
            if message.turn == turn {
                return previous;
            }
            previous = Some(message.turn);
        }
    }
    previous
}

/// The matter paths the turn's tool calls named.
fn turn_paths(log: &[Envelope], turn: TurnId) -> BTreeSet<MatterPath> {
    let mut tools: BTreeSet<ToolCallId> = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for envelope in log {
        if let DomainEvent::ToolCallStarted(started) = &envelope.event
            && started.turn == turn
        {
            tools.insert(started.tool_call);
            updates::note_paths(&mut paths, &started.locations, &started.content);
        } else if let DomainEvent::ToolCallUpdated(updated) = &envelope.event
            && tools.contains(&updated.tool_call)
        {
            let locations = updated.patch.locations.as_deref().unwrap_or_default();
            let content = updated.patch.content.as_deref().unwrap_or_default();
            updates::note_paths(&mut paths, locations, content);
        }
    }
    paths
}

fn after_turn(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    log: &[Envelope],
    pending: Pending,
) {
    let tool_paths = turn_paths(log, turn);
    let kept = pending.clone();
    let prepared = (|| -> Result<(clauseo_protocol::MatterId, TurnMeta), Error> {
        let matter = matter_of(actor, thread)?;
        let now = actor.now().wall;
        let meta = TurnMeta {
            checkpoint: actor.ids.mint(now)?,
            thread,
            turn,
            at: now,
        };
        Ok((matter, meta))
    })();
    let sent = prepared.and_then(|(matter, meta)| {
        let job = TurnJob {
            thread,
            kind: JobKind::Reconcile(Box::new(Repair { turn, pending })),
        };
        let after = MatterJob::AfterTurn { meta, tool_paths };
        actor
            .matter_worker(matter)?
            .send(after, MatterTag::Turn(job))?;
        actor.work.matter_jobs += 1;
        Ok(())
    });
    if let Err(error) = sent {
        let repair = Repair {
            turn,
            pending: kept,
        };
        on_after_turn(actor, thread, repair, Err(error));
    }
}

/// The after-turn checkpoint of an interrupted turn answered: the repair commit.
pub(super) fn on_after_turn(
    actor: &mut Actor,
    thread: ThreadId,
    repair: Repair,
    result: Result<Checkpoint, Error>,
) {
    let Repair { turn, pending } = repair;
    let mut close = TurnClose::new(turn, StopReason::Failed);
    close.expiries = pending.expiries();
    close.failure = Some((ErrorCode::ThreadInterrupted, None));
    if pending.attached {
        close.detach = Some(DetachReason::Closed);
    }
    let logged = matter_of(actor, thread).and_then(|matter| {
        match result {
            Ok(checkpoint) => end::after_events(&mut close, matter, thread, checkpoint)?,
            Err(error) => {
                tracing::warn!(%thread, %error, "an interrupted turn's checkpoint failed");
            }
        }
        close.send(actor, thread, Then::Nothing)
    });
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "an interrupted turn was not ended");
    }
    done(actor, thread);
}

impl Pending {
    /// The requests to expire `HostRestarted`.
    fn expiries(&self) -> Expiries {
        Expiries::Listed {
            permissions: self.permissions.clone(),
            elicitations: self.elicitations.clone(),
        }
    }
}

fn detach(actor: &mut Actor, out: &mut Out, pending: &Pending) -> Result<(), Error> {
    if pending.attached {
        let reason = DetachReason::Closed;
        out.push(actor, AgentSessionDetached { reason })?;
    }
    Ok(())
}

/// The repair is done; the stream may unload.
fn done(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.reconciling = false;
    }
    actor.release(StreamId::Thread(thread));
}
