//! `UndoStep`: the matter's writer must be free and is held during the undo; the step is read
//! from the thread's log; R8 undoes it on the matter's thread; one commit logs it.

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
use super::{JobKind, LogFor, TurnInput, TurnJob, end, lost, matter_of, state, steps};
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
/// is dropped and the undo tried once more with the same ids.
pub(super) fn on_undone(
    actor: &mut Actor,
    thread: ThreadId,
    mut ask: UndoAsk,
    result: Result<Undone, Error>,
) {
    match result {
        Ok(undone) => {
            let logged = log_undo(actor, thread, &ask, undone);
            if let Err(error) = logged {
                actor.finish_command(ask.ctx.id, Err(error));
            }
            end::release_writer(actor, ask.matter, thread);
        }
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

/// One commit: the outside-edits checkpoint and its event when R8 made one, the undo
/// checkpoint when R8 created it, `StepUndone`, the thread row and the receipt.
fn log_undo(
    actor: &mut Actor,
    thread: ThreadId,
    ask: &UndoAsk,
    undone: Undone,
) -> Result<(), Error> {
    let Undone {
        outside_edits,
        checkpoint,
        restored,
        kept,
        ..
    } = undone;
    let (ctx, matter, turn) = (ask.ctx, ask.matter, ask.turn);
    let internal = |error: clauseo_protocol::Error| Error::Internal(error.to_string());
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    if let Some(edits) = outside_edits
        && let CheckpointOutcome::Created(id) = edits.outcome
    {
        let recorded = CheckpointRecorded::new(id, CheckpointKind::OutsideEdits, None, None)
            .map_err(internal)?;
        out.push_matter(actor, matter, recorded)?;
        let event = OutsideEdits {
            checkpoint: id,
            files: edits.changes,
        };
        out.push_matter(actor, matter, event)?;
    }
    if let CheckpointOutcome::Created(id) = checkpoint.outcome {
        let recorded = CheckpointRecorded::new(id, CheckpointKind::Undo, Some(thread), Some(turn))
            .map_err(internal)?;
        out.push_matter(actor, matter, recorded)?;
    }
    let undo = checkpoint.outcome.id();
    out.push(
        actor,
        StepUndone {
            turn,
            checkpoint: undo,
            restored: restored.clone(),
            kept,
        },
    )?;
    let now = actor.now().wall;
    let entry = actor
        .threads
        .get_mut(&thread)
        .ok_or_else(|| lost("a thread"))?;
    entry.row.updated = now;
    out.draft.thread_row(entry.row.clone());
    let outcome = CommandOutcome::StepUndone { checkpoint: undo };
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), outcome, now));
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Nothing,
    };
    out.send_tagged(actor, tag)?;
    // The next prompt starts with the note on these files.
    steps::ensure_note(actor, thread);
    if let Some(note) = state(actor, thread).and_then(|state| state.note.as_mut()) {
        for path in restored {
            if !note.contains(&path) {
                note.push(path);
            }
        }
    }
    Ok(())
}
