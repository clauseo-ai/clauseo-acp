//! `RestoreVersion`: the matter's writer must be free and is held for the whole restore; the
//! version must be one of the matter's; R8 restores it on the matter's thread; one commit of
//! the matter's stream logs it. A restore belongs to no thread, so the writer's holder is
//! [`Holder::Restore`] and the commit carries no thread event. The writer and the note wait
//! until that commit is durable; a failed one is sent once more when the matter's stream
//! drains, as an undo's is.

use clauseo_counts::schema::Feature;
use clauseo_matters::{CheckpointOutcome, Restored};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, CheckpointRecorded, CommandOutcome, ErrorCode, MatterId,
    MatterPath, RestoreRequest, StreamId, VersionRestored,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::input::MatterTag;
use super::super::query::own_versions;
use super::super::{Actor, Tag, counting};
use super::{Holder, end, known, lost, undo};
use crate::error::Error;
use crate::matter_worker::{MatterJob, RestoreDone, RestoreJob};
use crate::pipeline::{Draft, MatterEvent};

/// A restore between its checks and R8's answer.
#[derive(Debug)]
pub(crate) struct RestoreAsk {
    ctx: CommandCtx,
    matter: MatterId,
    from: CheckpointId,
    scope: RestoreRequest,
}

/// Takes the matter's writer (`matter.turn_running` while a turn or an undo holds it or turns
/// wait for it), then checks the version against the matter's log.
pub(crate) fn restore_version(
    actor: &mut Actor,
    ctx: CommandCtx,
    matter: MatterId,
    from: CheckpointId,
    scope: RestoreRequest,
) {
    if let Err(error) = take_writer(actor, matter) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let ask = RestoreAsk {
        ctx,
        matter,
        from,
        scope,
    };
    let stream = StreamId::Matter(matter);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let sent = loaded
                .and_then(|()| own_versions(actor, matter, &[from, from]))
                .and_then(|()| send_restore(actor, ask));
            if let Err(error) = sent {
                give_up(actor, matter, ctx, error);
            }
            actor.release(stream);
        }),
    );
}

fn take_writer(actor: &mut Actor, matter: MatterId) -> Result<(), Error> {
    actor.check_matter(matter)?;
    let turns = &mut actor
        .matters
        .get_mut(&matter)
        .ok_or_else(|| lost("a matter"))?
        .turns;
    let message = match turns.holder {
        Some(Holder::Restore) => "a restore is running in this matter; wait for it",
        Some(Holder::Relocate) => "the matter's folder is being located; wait for it",
        Some(Holder::Thread(_)) => "a turn is running in this matter; wait for it or cancel it",
        None if turns.queue.is_empty() => {
            turns.holder = Some(Holder::Restore);
            return Ok(());
        }
        None => "turns are waiting in this matter; wait for them or cancel them",
    };
    Err(Error::refused(ErrorCode::MatterTurnRunning, message))
}

/// Mints the two checkpoint ids and sends the restore to the matter's thread.
fn send_restore(actor: &mut Actor, ask: RestoreAsk) -> Result<(), Error> {
    let now = actor.now().wall;
    let job = RestoreJob {
        from: ask.from,
        scope: ask.scope.clone(),
        outside_edits: actor.ids.mint(now)?,
        checkpoint: actor.ids.mint(now)?,
        at: now,
    };
    let matter = ask.matter;
    actor
        .matter_worker(matter)?
        .send(MatterJob::Restore(job), MatterTag::Restore(Box::new(ask)))?;
    actor.work.matter_jobs += 1;
    Ok(())
}

/// Lets the writer go and answers the command with `error`.
fn give_up(actor: &mut Actor, matter: MatterId, ctx: CommandCtx, error: Error) {
    end::pass_writer(actor, matter);
    actor.finish_command(ctx.id, Err(error));
}

/// The commit that logs a restore R8 has done, kept until it is durable so it can be sent once
/// more: R8 has already written the files.
#[derive(Debug)]
pub(crate) struct RestoreLog {
    ctx: CommandCtx,
    /// The outside-edits checkpoint and its event, the restore checkpoint, `VersionRestored`.
    events: Vec<MatterEvent>,
    /// The files R8 wrote, for the next prompt's note.
    written: Vec<MatterPath>,
    outcome: CommandOutcome,
    /// That commit failed; the unblock sends it again.
    failed: bool,
    /// The one retry was sent.
    retried: bool,
}

/// R8 answered.
pub(crate) fn on_restored(actor: &mut Actor, ask: &RestoreAsk, result: Result<RestoreDone, Error>) {
    let (ctx, matter) = (ask.ctx, ask.matter);
    match result.and_then(|done| restore_log(actor, ask, done)) {
        Ok(log) => send_log(actor, matter, log),
        Err(error) => give_up(actor, matter, ctx, error),
    }
}

/// The events of the commit logging `restored`: the outside-edits checkpoint and its event when
/// R8 made one; when R8 wrote files, `CheckpointRecorded { Restore }` and `VersionRestored`.
/// A restore with nothing to write (R8's `Reused`) logs only the outside edits and answers
/// `VersionRestored` with the reused id, as `UndoStep` answers `StepUndone` with the
/// checkpoint it reused: the outcome alone does not tell a restore that wrote nothing from
/// one that did; the absence of the `VersionRestored` event does. A reused id the matter's log
/// does not hold (its commit never landed: a crash, or a log commit that failed twice) is
/// recorded first, with its row, so the outcome never names an unrecorded checkpoint.
fn restore_log(actor: &Actor, ask: &RestoreAsk, done: RestoreDone) -> Result<RestoreLog, Error> {
    let RestoreDone { restored, reused } = done;
    let Restored {
        outside_edits,
        checkpoint,
        written,
        ..
    } = restored;
    let mut events = undo::outside_edits_events(outside_edits)?;
    let id = checkpoint.outcome.id();
    if events.is_empty()
        && let CheckpointOutcome::Reused(id) = checkpoint.outcome
        && !known::recorded(actor, ask.matter, id)
    {
        if let Some(entry) = reused {
            let recorded = CheckpointRecorded::new(id, entry.kind, entry.thread, entry.turn)
                .map_err(|error| Error::Internal(error.to_string()))?;
            events.push(MatterEvent::checkpoint(recorded, checkpoint.commit.clone()));
        } else {
            tracing::error!(%id, "R8 reused a checkpoint its timeline does not hold");
        }
    }
    if let CheckpointOutcome::Created(id) = checkpoint.outcome {
        let recorded = CheckpointRecorded::new(id, CheckpointKind::Restore, None, None)
            .map_err(|error| Error::Internal(error.to_string()))?;
        events.push(MatterEvent::checkpoint(recorded, checkpoint.commit));
        events.push(MatterEvent::plain(VersionRestored {
            from: ask.from,
            checkpoint: id,
            scope: ask.scope.clone().into(),
        }));
    }
    Ok(RestoreLog {
        ctx: ask.ctx,
        events,
        written,
        outcome: CommandOutcome::VersionRestored { checkpoint: id },
        failed: false,
        retried: false,
    })
}

/// One commit of the matter's stream: the events, their checkpoint rows and the receipt. The
/// first send answers the command; the retry carries the receipt again, so the same command
/// sent again finds the stored outcome.
fn log_commit(actor: &mut Actor, matter: MatterId, log: &RestoreLog) -> Result<(), Error> {
    let ctx = log.ctx;
    let now = actor.now().wall;
    let mut draft = Draft::new();
    for event in &log.events {
        actor.pipeline.push_matter(
            &mut draft,
            matter,
            event.clone(),
            Some(ctx.id),
            now,
            &mut actor.ids,
        )?;
    }
    let stream = StreamId::Matter(matter);
    draft.receipt(ctx.receipt(Some(stream), log.outcome, now));
    let tag = if log.retried {
        Tag::Restore(matter)
    } else {
        Tag::Command {
            id: ctx.id,
            outcome: log.outcome,
            after: AfterCommand::Restore(matter),
        }
    };
    actor.send_commit(draft, tag).map(|_ticket| ())
}

/// Sends the commit logging the restore and keeps it on the matter until it returns.
fn send_log(actor: &mut Actor, matter: MatterId, log: RestoreLog) {
    let sent = log_commit(actor, matter, &log);
    let ctx = log.ctx;
    let Some(entry) = actor.matters.get_mut(&matter) else {
        actor.finish_command(ctx.id, Err(lost("a matter")));
        return;
    };
    entry.turns.restore = Some(Box::new(log));
    if let Err(error) = sent {
        tracing::warn!(%matter, %error, "a restore was not logged");
        actor.finish_command(ctx.id, Err(error));
        on_failed(actor, matter);
        if actor.pipeline.blocked(StreamId::Matter(matter)).is_none() {
            retry(actor, matter);
        }
    }
}

/// The commit logging the restore returned: durable, the threads of the matter note the files
/// put back and the writer passes on; failed, the unblock sends it once more.
pub(crate) fn committed(actor: &mut Actor, matter: MatterId, durable: bool) {
    if !durable {
        on_failed(actor, matter);
        return;
    }
    let Some(log) = actor
        .matters
        .get_mut(&matter)
        .and_then(|entry| entry.turns.restore.take())
    else {
        return;
    };
    counting::feature_used(actor, Feature::Restore);
    note_restored(actor, matter, &log.written);
    end::pass_writer(actor, matter);
}

/// Adds `written` to the next prompt's note of every thread of `matter`. Kept in memory only:
/// `VersionRestored` does not list the files written, so a restart before a thread's next
/// prompt loses this part of its note. No turn of the matter can start between the restore
/// and this call (the restore holds the writer), so the note follows the matter's timeline.
fn note_restored(actor: &mut Actor, matter: MatterId, written: &[MatterPath]) {
    if written.is_empty() {
        return;
    }
    for entry in actor.threads.values_mut() {
        if entry.row.matter == matter {
            let note = &mut entry.turn.restored;
            for path in written {
                if !note.contains(path) {
                    note.push(path.clone());
                }
            }
        }
    }
}

/// The commit logging the restore failed: the unblock sends it once more; a retry that fails
/// too lets the writer go, with the restore done on disk and not in the log.
fn on_failed(actor: &mut Actor, matter: MatterId) {
    let Some(log) = actor
        .matters
        .get_mut(&matter)
        .and_then(|entry| entry.turns.restore.as_mut())
    else {
        return;
    };
    if !log.retried {
        log.failed = true;
        return;
    }
    tracing::warn!(%matter, "a restore failed to log twice; its files are written on disk");
    let dropped = actor
        .matters
        .get_mut(&matter)
        .and_then(|entry| entry.turns.restore.take());
    // The files are restored on disk whether or not the log says so: the agent is told.
    if let Some(log) = dropped {
        note_restored(actor, matter, &log.written);
    }
    end::pass_writer(actor, matter);
}

/// The matter's stream drained after a failure: send a failed restore commit once more.
pub(super) fn retry(actor: &mut Actor, matter: MatterId) {
    let Some(mut log) = actor.matters.get_mut(&matter).and_then(|entry| {
        entry
            .turns
            .restore
            .take_if(|log| log.failed && !log.retried)
    }) else {
        return;
    };
    log.retried = true;
    let sent = log_commit(actor, matter, &log);
    if let Some(entry) = actor.matters.get_mut(&matter) {
        entry.turns.restore = Some(log);
    }
    if let Err(error) = sent {
        tracing::warn!(%matter, %error, "a restore was not logged");
        on_failed(actor, matter);
    }
}
