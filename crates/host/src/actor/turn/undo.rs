//! `UndoStep`: the matter's writer must be free and is held during the undo; the step is read
//! from the thread's log; R8 undoes it on the matter's thread; one commit logs it. The writer
//! and the undo note wait until that commit is durable; a failed one is sent once more when the
//! stream drains, as the end of a turn is.

use std::collections::BTreeSet;

use clauseo_matters::{CheckpointOutcome, Undone};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, CheckpointRecorded, CommandOutcome, DomainEvent, Envelope,
    ErrorCode, FileChoice, FileDecision, MatterId, MatterPath, OutsideEdits, StepUndone, StreamId,
    ThreadId, TurnId,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::input::{Input, MatterTag};
use super::super::{Actor, Tag};
use super::out::Out;
use super::{
    JobKind, LogFor, Then, TurnInput, TurnJob, TurnTag, end, failure, lost, matter_of, prompt,
    state,
};
use crate::error::Error;
use crate::matter_worker::{MatterJob, UndoJob};

/// An undo between its checks and its commit.
#[derive(Debug)]
pub(crate) struct UndoAsk {
    ctx: CommandCtx,
    matter: MatterId,
    turn: TurnId,
    /// The lawyer's decisions.
    files: Vec<FileDecision>,
    /// The job sent to R8, once the step is known.
    job: Option<UndoJob>,
    /// The paths the host filled with `Keep`.
    filled: Vec<MatterPath>,
    /// Whether R8 already refused one of the filled decisions.
    retried: bool,
}

/// Takes the matter's writer, then reads the thread's log for the step.
pub(super) fn undo_step(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
    files: Vec<FileDecision>,
) -> Result<(), Error> {
    let matter = matter_of(actor, thread)?;
    let busy = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.turn.current.is_some());
    let turns = &mut actor
        .matters
        .get_mut(&matter)
        .ok_or_else(|| lost("a matter"))?
        .turns;
    if busy || turns.holder.is_some() {
        return Err(Error::refused(
            ErrorCode::MatterTurnRunning,
            "a turn is running in this matter; wait for it or cancel it",
        ));
    }
    turns.holder = Some(thread);
    let ask = UndoAsk {
        ctx,
        matter,
        turn,
        files,
        job: None,
        filled: Vec::new(),
        retried: false,
    };
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = super::super::streams::read_whole_log(&store, StreamId::Thread(thread))
            .await
            .map(|reopen| reopen.tail);
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::Undo(Box::new(ask)),
            result,
        })
    });
    Ok(())
}

/// The step as the log has it.
struct Step {
    before: CheckpointId,
    after: CheckpointId,
    paths: Vec<MatterPath>,
}

/// The step of `turn`: started, ended with a checkpoint, not undone; else `command.invalid`.
fn step_of(log: &[Envelope], turn: TurnId) -> Result<Step, Error> {
    let (mut before, mut after, mut undone) = (None, None, false);
    let mut paths = Vec::new();
    for envelope in log {
        let event = &envelope.event;
        if let DomainEvent::TurnStarted(started) = event
            && started.turn == turn
        {
            before = Some(started.checkpoint_before);
        } else if let DomainEvent::TurnEnded(ended) = event
            && ended.turn == turn
        {
            after = ended.checkpoint_after;
        } else if let DomainEvent::FilesChanged(changed) = event
            && changed.turn == turn
        {
            paths.extend(changed.files.iter().map(|file| file.path().clone()));
        } else if let DomainEvent::StepUndone(step) = event
            && step.turn == turn
        {
            undone = true;
        }
    }
    match (before, after, undone) {
        (Some(before), Some(after), false) => Ok(Step {
            before,
            after,
            paths,
        }),
        _ => Err(Error::refused(
            ErrorCode::CommandInvalid,
            format!("turn {turn} of this thread has no step to undo"),
        )),
    }
}

/// The log came back: build the decisions (`Keep` for each path the lawyer left out) and
/// send the undo.
pub(super) fn on_log(
    actor: &mut Actor,
    thread: ThreadId,
    mut ask: UndoAsk,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let sent = result
        .map_err(Error::Store)
        .and_then(|log| step_of(&log, ask.turn))
        .and_then(|step| {
            let named: BTreeSet<&str> = ask.files.iter().map(|file| file.path.as_str()).collect();
            if let Some(stray) = ask
                .files
                .iter()
                .find(|file| !step.paths.contains(&file.path))
            {
                return Err(Error::refused(
                    ErrorCode::CommandInvalid,
                    format!("{} is not a file of this step", stray.path),
                ));
            }
            let mut decisions = ask.files.clone();
            for path in &step.paths {
                if !named.contains(path.as_str()) {
                    ask.filled.push(path.clone());
                    decisions.push(FileDecision {
                        path: path.clone(),
                        choice: FileChoice::Keep,
                    });
                }
            }
            let now = actor.now().wall;
            ask.job = Some(UndoJob {
                before: step.before,
                after: step.after,
                decisions,
                thread: Some(thread),
                turn: Some(ask.turn),
                outside_edits: actor.ids.mint(now)?,
                checkpoint: actor.ids.mint(now)?,
                at: now,
            });
            Ok(())
        });
    match sent {
        Ok(()) => send_undo(actor, thread, ask),
        Err(error) => give_up(actor, thread, &ask, error),
    }
}

fn send_undo(actor: &mut Actor, thread: ThreadId, ask: UndoAsk) {
    let (ctx, matter) = (ask.ctx, ask.matter);
    let Some(job) = ask.job.clone() else {
        give_up(actor, thread, &ask, lost("an undo job"));
        return;
    };
    let tag = MatterTag::Turn(TurnJob {
        thread,
        kind: JobKind::Undo(Box::new(ask)),
    });
    let sent = actor
        .matter_worker(matter)
        .and_then(|worker| worker.send(MatterJob::Undo(job), tag));
    match sent {
        Ok(()) => actor.work.matter_jobs += 1,
        Err(error) => {
            end::release_writer(actor, matter, thread);
            actor.finish_command(ctx.id, Err(error));
        }
    }
}

fn give_up(actor: &mut Actor, thread: ThreadId, ask: &UndoAsk, error: Error) {
    end::release_writer(actor, ask.matter, thread);
    actor.finish_command(ask.ctx.id, Err(error));
}

/// R8 answered. A refused `Keep` the host filled (a deletion now cloud-only or over the cap)
/// is dropped and the undo tried once more with the same ids. The writer stays held until the
/// commit logging the undo is durable (or its one retry has returned).
pub(super) fn on_undone(
    actor: &mut Actor,
    thread: ThreadId,
    mut ask: UndoAsk,
    result: Result<Undone, Error>,
) {
    match result {
        Ok(undone) => match undo_log(thread, &ask, undone) {
            Ok(log) => send_log(actor, thread, log),
            Err(error) => give_up(actor, thread, &ask, error),
        },
        Err(Error::Matters(clauseo_matters::Error::UnexpectedDecision { path }))
            if !ask.retried && ask.filled.contains(&path) =>
        {
            ask.retried = true;
            if let Some(job) = &mut ask.job {
                job.decisions.retain(|decision| decision.path != path);
            }
            send_undo(actor, thread, ask);
        }
        Err(error) => give_up(actor, thread, &ask, error),
    }
}

/// The commit that logs an undo R8 has done, kept until it is durable so it can be sent once
/// more: R8 has already put the files back.
#[derive(Debug)]
pub(crate) struct UndoLog {
    ctx: CommandCtx,
    matter: MatterId,
    /// The matter's events: the outside-edits checkpoint and its event, the undo checkpoint.
    matter_events: Vec<DomainEvent>,
    step: StepUndone,
    outcome: CommandOutcome,
    /// That commit failed; the unblock sends it again.
    failed: bool,
    /// The one retry was sent.
    retried: bool,
}

/// The events of the commit logging `undone`: the outside-edits checkpoint and its event when
/// R8 made one, the undo checkpoint when R8 created it, then `StepUndone`.
fn undo_log(thread: ThreadId, ask: &UndoAsk, undone: Undone) -> Result<UndoLog, Error> {
    let Undone {
        outside_edits,
        checkpoint,
        restored,
        kept,
        ..
    } = undone;
    let (matter, turn) = (ask.matter, ask.turn);
    let internal = |error: clauseo_protocol::Error| Error::Internal(error.to_string());
    let mut matter_events: Vec<DomainEvent> = Vec::new();
    if let Some(edits) = outside_edits
        && let CheckpointOutcome::Created(id) = edits.outcome
    {
        let recorded = CheckpointRecorded::new(id, CheckpointKind::OutsideEdits, None, None)
            .map_err(internal)?;
        matter_events.push(recorded.into());
        let event = OutsideEdits {
            checkpoint: id,
            files: edits.changes,
        };
        matter_events.push(event.into());
    }
    if let CheckpointOutcome::Created(id) = checkpoint.outcome {
        let recorded = CheckpointRecorded::new(id, CheckpointKind::Undo, Some(thread), Some(turn))
            .map_err(internal)?;
        matter_events.push(recorded.into());
    }
    let undo = checkpoint.outcome.id();
    Ok(UndoLog {
        ctx: ask.ctx,
        matter,
        matter_events,
        step: StepUndone {
            turn,
            checkpoint: undo,
            restored,
            kept,
        },
        outcome: CommandOutcome::StepUndone { checkpoint: undo },
        failed: false,
        retried: false,
    })
}

/// One commit: the matter's events, `StepUndone`, the thread row and the receipt. The first
/// send answers the command; the retry carries the receipt again, so the same command sent
/// again finds the stored outcome.
fn log_commit(actor: &mut Actor, thread: ThreadId, log: &UndoLog) -> Result<(), Error> {
    let ctx = log.ctx;
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    for event in &log.matter_events {
        out.push_matter(actor, log.matter, event.clone())?;
    }
    out.push(actor, log.step.clone())?;
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), log.outcome, now));
    let then = TurnTag {
        thread,
        then: Then::Undone,
    };
    if log.retried {
        return out.send_tagged(actor, Tag::Turn(then));
    }
    let tag = Tag::Command {
        id: ctx.id,
        outcome: log.outcome,
        after: AfterCommand::Turn(then),
    };
    out.send_tagged(actor, tag)
}

/// Sends the commit logging the undo and keeps it on the thread until it returns.
fn send_log(actor: &mut Actor, thread: ThreadId, log: UndoLog) {
    let sent = log_commit(actor, thread, &log);
    let (ctx, matter) = (log.ctx, log.matter);
    let Some(state) = state(actor, thread) else {
        end::release_writer(actor, matter, thread);
        return;
    };
    state.undo = Some(Box::new(log));
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "an undo was not logged");
        actor.finish_command(ctx.id, Err(error));
        on_failed(actor, thread);
        if !failure::blocked(actor, thread) {
            retry(actor, thread);
        }
    }
}

/// The commit logging the undo is durable (or its retry returned): the next prompt's note
/// names the files put back, and the writer passes on.
pub(super) fn on_logged(actor: &mut Actor, thread: ThreadId) {
    let Some(log) = state(actor, thread).and_then(|state| state.undo.take()) else {
        return;
    };
    prompt::ensure_note(actor, thread);
    if let Some(note) = state(actor, thread).and_then(|state| state.note.as_mut()) {
        for path in log.step.restored {
            if !note.contains(&path) {
                note.push(path);
            }
        }
    }
    end::release_writer(actor, log.matter, thread);
}

/// The commit logging the undo failed: the unblock sends it once more; a retry that fails too
/// lets the writer go, with the undo done on disk and not in the log.
pub(super) fn on_failed(actor: &mut Actor, thread: ThreadId) {
    let Some(log) = state(actor, thread).and_then(|state| state.undo.as_mut()) else {
        return;
    };
    if !log.retried {
        log.failed = true;
        return;
    }
    let matter = log.matter;
    tracing::warn!(%thread, "an undo failed to log twice; its files are back on disk");
    if let Some(state) = state(actor, thread) {
        state.undo = None;
    }
    end::release_writer(actor, matter, thread);
}

/// The thread's stream drained after a failure: send a failed undo commit once more.
pub(super) fn retry(actor: &mut Actor, thread: ThreadId) {
    let Some(log) = state(actor, thread)
        .and_then(|state| state.undo.as_mut())
        .filter(|log| log.failed && !log.retried)
    else {
        return;
    };
    log.retried = true;
    let Some(log) = state(actor, thread).and_then(|state| state.undo.take()) else {
        return;
    };
    let sent = log_commit(actor, thread, &log);
    if let Some(state) = state(actor, thread) {
        state.undo = Some(log);
    }
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "an undo was not logged");
        on_failed(actor, thread);
    }
}
