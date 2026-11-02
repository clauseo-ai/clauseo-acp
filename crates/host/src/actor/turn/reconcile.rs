//! Start-up reconciliation: a thread whose log ends inside a turn (the host stopped without
//! ending it) gets one commit. Pending requests expire `HostRestarted`; a started turn gets an
//! after-turn checkpoint and ends `TurnFailed { thread.interrupted }` (a checkpoint R8 made
//! whose record the log lost is recorded with the turn's changes, `known.rs`); a turn that never
//! started stays queued, held behind its own thread; an attached session is detached. A turn
//! with a `TurnEnded` is never touched, so each repair happens once, with one exception: a
//! thread whose last step stopped `AtLimit` with no `TurnStoppedAtLimit` for its turn (the read
//! that was to give its reset time was lost with the restart, or its commit failed) gets one,
//! with no window or reset time and the reason `PlanWindow`, in a commit of its own. Both are
//! read from the loaded state (a snapshot keeps its steps and limit cards), and once that event
//! is logged the card exists, so this too happens once. It runs only at start-up, when no
//! pending stop of `limits.rs` can exist.
//!
//! A turn whose end ran but was never logged (it failed twice) freed the writer, so other
//! threads' turns in the matter may have run, and one may still be open too. The started turns
//! of a matter are repaired once every thread of the matter that is not archived has been
//! looked at, and only the one that started last may take a new after-turn checkpoint; each
//! earlier one takes its own from R8's timeline or an empty step (`MatterJob::Repair`), so no
//! repair takes another turn's changes, in whatever order the threads load. When a thread of
//! the matter could not be loaded or its log read, which turn started last is not known, so
//! none may take a new checkpoint: a lone crashed turn there gets an empty step, and its
//! changes are logged as outside edits by the next turn's before-turn checkpoint.
//!
//! This holds within one start, not across starts: if a thread fails to load and other turns
//! then run in the matter, a later start that repairs that thread's turn as the only open one
//! takes a new checkpoint holding their changes (follow-up: refuse a new checkpoint when the
//! matter has a `TurnStarted` newer than the turn's).

use std::cmp::Reverse;
use std::collections::BTreeSet;

use clauseo_matters::TurnMeta;
use clauseo_protocol::{
    AgentSessionDetached, CheckpointId, DetachReason, DomainEvent, ElicitationId, ElicitationState,
    Envelope, ErrorCode, EventId, LimitStopReason, PermissionRequestId, PermissionState,
    StopReason, StreamId, ThreadId, ThreadStatus, ToolCallId, TurnId, TurnQueued,
    TurnStoppedAtLimit, UserMessageStatus, ViewEntry,
};
use clauseo_reducer::ThreadState;

use super::super::Actor;
use super::super::input::{Input, MatterTag};
use super::close::{Expiries, TurnClose, log_limit_stop};
use super::out::Out;
use super::{JobKind, LogFor, Then, TurnInput, TurnJob, end, matter_of, state};
use crate::error::Error;
use crate::matter_worker::{MatterJob, TurnCheckpoint};
use crate::sent::{SentView, TurnPaths};

/// A started turn's repair, while its after-turn checkpoint runs.
#[derive(Debug)]
pub(crate) struct Repair {
    turn: TurnId,
    pending: Pending,
}

/// A matter's start-up repairs of started turns, held until every thread of the matter has been
/// looked at: only then is it known which open turn started last.
#[derive(Debug, Default)]
pub(crate) struct Repairs {
    /// Threads of the matter start-up has not looked at yet.
    left: usize,
    /// A thread of the matter could not be loaded or its log read: it may hold an open turn
    /// that started after every waiting one.
    incomplete: bool,
    /// The started turns found so far.
    waiting: Vec<Waiting>,
}

/// A started turn's repair, waiting for the rest of its matter.
#[derive(Debug)]
struct Waiting {
    thread: ThreadId,
    turn: TurnId,
    /// Its `TurnStarted`'s event id: a v7 UUID the host mints in order within a run
    /// (`uuid::ContextV7` keeps the order when the clock goes back), so the largest is the
    /// turn that started last. Across runs the order follows the wall clock: a turn left open
    /// by a failed repair commit, then a clock set back before a later turn starts, could
    /// reverse it (follow-up: seed the id source from the newest stored id).
    started: Option<EventId>,
    tool_paths: TurnPaths,
    before: Option<CheckpointId>,
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
    let unstopped = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(stop_not_logged);
    if let Some(turn) = unstopped {
        let stopped = TurnStoppedAtLimit {
            turn,
            window: None,
            resets_at: None,
            reason: LimitStopReason::PlanWindow,
        };
        log_limit_stop(actor, thread, stopped);
    }
    let needs = actor
        .loaded(stream)
        .and_then(|loaded| loaded.state.as_thread())
        .is_some_and(|state| open_turn(state).is_some() || has_pending(state));
    if !needs {
        looked_at(actor, thread);
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

/// The turn of the thread's last step when that step stopped `AtLimit` and the turn has no
/// `TurnStoppedAtLimit`.
fn stop_not_logged(state: &ThreadState) -> Option<TurnId> {
    let mut last = None;
    let mut cards: BTreeSet<TurnId> = BTreeSet::new();
    for entry in state.entries().values() {
        if let ViewEntry::Step(step) = entry {
            last = Some((step.turn, step.stop));
        } else if let ViewEntry::LimitStop(card) = entry {
            cards.insert(card.turn);
        }
    }
    let (turn, stop) = last?;
    (stop == StopReason::AtLimit && !cards.contains(&turn)).then_some(turn)
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
            skipped(actor, thread);
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
        Some((turn, true)) => hold(actor, thread, turn, &log, pending),
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
    looked_at(actor, thread);
}

/// Start-up will look at `thread`: its matter's repairs wait for it.
pub(super) fn expect(actor: &mut Actor, thread: ThreadId) {
    if let Some(repairs) = repairs(actor, thread) {
        repairs.left += 1;
    }
}

/// Start-up could not load `thread` or read its log: its matter's repairs are incomplete.
pub(super) fn skipped(actor: &mut Actor, thread: ThreadId) {
    if let Some(repairs) = repairs(actor, thread) {
        repairs.incomplete = true;
    }
    looked_at(actor, thread);
}

/// Start-up has looked at `thread`; the last thread of a matter sends the matter's repairs.
pub(super) fn looked_at(actor: &mut Actor, thread: ThreadId) {
    let Some(repairs) = repairs(actor, thread) else {
        return;
    };
    repairs.left = repairs.left.saturating_sub(1);
    if repairs.left > 0 {
        return;
    }
    let complete = !repairs.incomplete;
    let mut waiting = std::mem::take(&mut repairs.waiting);
    // The turn that started last first.
    waiting.sort_by_key(|repair| Reverse(repair.started));
    for (n, repair) in waiting.into_iter().enumerate() {
        after_turn(actor, repair, complete && n == 0);
    }
}

fn repairs(actor: &mut Actor, thread: ThreadId) -> Option<&mut Repairs> {
    let matter = matter_of(actor, thread).ok()?;
    actor
        .matters
        .get_mut(&matter)
        .map(|entry| &mut entry.turns.repairs)
}

/// A started turn's repair waits for the rest of its matter ([`looked_at`] sends it); one
/// whose matter is not known is sent now, and fails as it would have.
fn hold(actor: &mut Actor, thread: ThreadId, turn: TurnId, log: &[Envelope], pending: Pending) {
    let repair = Waiting {
        thread,
        turn,
        started: turn_started(log, turn),
        tool_paths: turn_paths(log, turn),
        before: checkpoint_before(log, turn),
        pending,
    };
    match repairs(actor, thread) {
        Some(repairs) => repairs.waiting.push(repair),
        None => after_turn(actor, repair, true),
    }
}

/// The event id of `turn`'s `TurnStarted`.
fn turn_started(log: &[Envelope], turn: TurnId) -> Option<EventId> {
    log.iter().find_map(|envelope| {
        if let DomainEvent::TurnStarted(started) = &envelope.event
            && started.turn == turn
        {
            Some(envelope.event_id)
        } else {
            None
        }
    })
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

/// The before-turn checkpoint `turn`'s `TurnStarted` names.
fn checkpoint_before(log: &[Envelope], turn: TurnId) -> Option<CheckpointId> {
    log.iter().find_map(|envelope| {
        if let DomainEvent::TurnStarted(started) = &envelope.event
            && started.turn == turn
        {
            Some(started.checkpoint_before)
        } else {
            None
        }
    })
}

/// The places the turn's tool calls named.
fn turn_paths(log: &[Envelope], turn: TurnId) -> TurnPaths {
    let mut tools: BTreeSet<ToolCallId> = BTreeSet::new();
    let mut paths = TurnPaths::default();
    for envelope in log {
        if let DomainEvent::ToolCallStarted(started) = &envelope.event
            && started.turn == turn
        {
            tools.insert(started.tool_call);
            paths.note(&started.locations, &started.content);
        } else if let DomainEvent::ToolCallUpdated(updated) = &envelope.event
            && tools.contains(&updated.tool_call)
        {
            let locations = updated.patch.locations.as_deref().unwrap_or_default();
            let content = updated.patch.content.as_deref().unwrap_or_default();
            paths.note(locations, content);
        }
    }
    paths
}

/// Sends the repair's checkpoint job; `last` when no other open turn of the matter started after
/// this one.
fn after_turn(actor: &mut Actor, repair: Waiting, last: bool) {
    let Waiting {
        thread,
        turn,
        tool_paths,
        before,
        pending,
        ..
    } = repair;
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
        let after = MatterJob::Repair {
            meta,
            tool_paths,
            before,
            last,
        };
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
    result: Result<TurnCheckpoint, Error>,
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
            Ok(done) => end::after_events(actor, &mut close, matter, thread, done)?,
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
