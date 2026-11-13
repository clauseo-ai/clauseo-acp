//! `DecideRevisions`, the actor's side, as a restore's: the matter's writer is taken as
//! `RestoreVersion` takes it (`matter.turn_running` while a turn, an undo or a restore holds
//! it, or turns wait), the edit and the write run on the matter's thread
//! (`crate::revisions`), and one commit of the matter's stream logs the outside edits, if any,
//! and the `revisions` checkpoint, through the restore's [`RestoreLog`] and its one retry. The
//! writer passes on once that commit returns.
//!
//! No next-prompt note: the restore's note tells the agent files went back to an earlier
//! version, which a decision is not. No usage count: the counts have no feature for it.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_matters::{CheckpointOutcome, CommitId, Replaced, TimelineEntry};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, CheckpointRecorded, CommandOutcome, DecideRevisions, MatterId,
    StreamId,
};

use super::super::super::Actor;
use super::super::super::commands::CommandCtx;
use super::super::super::input::MatterTag;
use super::super::{known, undo};
use super::{RestoreLog, give_up, send_log, take_writer};
use crate::error::Error;
use crate::log_file::Cause;
use crate::matter_worker::{MatterJob, MatterReply};
use crate::pipeline::MatterEvent;
use crate::revisions::{Act, Decided, Ids, RevisionsDone, RevisionsJob};

/// A decision between its checks and the matter thread's answer.
#[derive(Debug)]
pub(crate) struct RevisionsAsk {
    ctx: CommandCtx,
    matter: MatterId,
}

/// Takes the matter's writer, then sends the decision to the matter's thread with the two
/// checkpoint ids it may take.
pub(crate) fn decide_revisions(
    actor: &mut Actor,
    ctx: CommandCtx,
    decide: DecideRevisions,
    stetdoc: PathBuf,
) {
    let matter = decide.matter;
    if let Err(error) = take_writer(actor, matter) {
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let stream = StreamId::Matter(matter);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            if let Err(error) = loaded.and_then(|()| send(actor, ctx, decide, stetdoc)) {
                give_up(actor, matter, ctx, error);
            }
            actor.release(stream);
        }),
    );
}

fn send(
    actor: &mut Actor,
    ctx: CommandCtx,
    decide: DecideRevisions,
    stetdoc: PathBuf,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let matter = decide.matter;
    let job = RevisionsJob {
        path: decide.path,
        stetdoc,
        work: super::super::super::revisions::work_folder(actor, matter),
        clock: Arc::clone(&actor.clock),
        act: Act::Decide {
            version: decide.version,
            ids: if decide.all {
                Ids::All
            } else {
                Ids::These(decide.ids)
            },
            decision: decide.decision,
            outside_edits: actor.ids.mint(now)?,
            checkpoint: actor.ids.mint(now)?,
            at: now,
        },
    };
    let ask = RevisionsAsk { ctx, matter };
    actor.matter_worker(matter)?.send(
        MatterJob::Revisions(Box::new(job)),
        MatterTag::Revisions(Box::new(ask)),
    )?;
    actor.work.matter_jobs += 1;
    Ok(())
}

/// The matter's thread answered: log what it wrote, or give the writer back with its refusal.
pub(crate) fn on_decided(actor: &mut Actor, ask: &RevisionsAsk, reply: MatterReply) {
    let result = if let MatterReply::Revisions(done) = reply {
        match *done {
            RevisionsDone::Decided(result) => result,
            RevisionsDone::Listed(_) => {
                Err(Error::Internal("a decision got a list's reply".to_owned()))
            }
        }
    } else {
        Err(Error::Internal(format!(
            "a decision got another job's reply: {}",
            reply.name()
        )))
    };
    match result {
        Ok(decided) => {
            let log = log(actor, ask, *decided);
            send_log(actor, ask.matter, log);
        }
        Err(error) => give_up(actor, ask.matter, ask.ctx, error),
    }
}

/// The commit logging `decided`: the outside-edits checkpoint and its event when one was
/// taken, then `CheckpointRecorded { Revisions }` when the file changed. An id the outcome
/// names that this decision did not create (`before`, a reused `revisions` id) and the matter's
/// log does not hold is recorded first, with its row, as `restore_log` records a reused id: an
/// earlier attempt's outside-edits checkpoint whose log commit was dropped (the attempt failed
/// after taking it) is the retry's `before`, and the room's undo restores it.
///
/// The file is written by now, so nothing here fails the decision: an event that cannot be
/// built is left out and logged, and the answer has no `checkpoint` when the `revisions` one
/// was not taken or its event could not be built. The commit itself is the restore's: a failed
/// one answers the command with its error and is sent once more; one that fails twice lets the
/// writer go with the file written on disk and not in the log, as a restore's does.
fn log(actor: &Actor, ask: &RevisionsAsk, decided: Decided) -> RestoreLog {
    let Decided {
        replaced,
        before: before_entry,
        reused,
    } = decided;
    let Replaced {
        outside_edits,
        before,
        before_commit,
        checkpoint,
    } = replaced;
    let mut events = undo::outside_edits_events(outside_edits).unwrap_or_else(|error| {
        tracing::error!(error = %Cause(&error), "a decision's outside edits were not logged");
        Vec::new()
    });
    if events.is_empty() {
        late(
            actor,
            ask.matter,
            &mut events,
            before,
            before_entry,
            before_commit,
        );
    }
    let id = checkpoint.and_then(|checkpoint| match checkpoint.outcome {
        CheckpointOutcome::Reused(id) => {
            if id != before {
                late(
                    actor,
                    ask.matter,
                    &mut events,
                    id,
                    reused,
                    checkpoint.commit,
                );
            }
            Some(id)
        }
        CheckpointOutcome::Created(id) => {
            match CheckpointRecorded::new(id, CheckpointKind::Revisions, None, None) {
                Ok(recorded) => {
                    events.push(MatterEvent::checkpoint(recorded, checkpoint.commit));
                    Some(id)
                }
                Err(error) => {
                    tracing::error!(%error, %id, "a decision's checkpoint was not logged");
                    None
                }
            }
        }
    });
    if id.is_none() {
        tracing::warn!(matter = %ask.matter, "a decision was written with no checkpoint after it");
    }
    RestoreLog {
        ctx: ask.ctx,
        events,
        note: None,
        thread: None,
        outcome: CommandOutcome::RevisionsDecided {
            before,
            checkpoint: id,
        },
        feature: None,
        failed: false,
        retried: false,
    }
}

/// Pushes `CheckpointRecorded` for `id` (with `commit`'s row) when the matter's log does not
/// hold it, from its timeline `entry`; one that cannot be built is logged and left out.
fn late(
    actor: &Actor,
    matter: MatterId,
    events: &mut Vec<MatterEvent>,
    id: CheckpointId,
    entry: Option<TimelineEntry>,
    commit: CommitId,
) {
    if known::recorded(actor, matter, id) {
        return;
    }
    let Some(entry) = entry else {
        tracing::error!(%id, "a decision names a checkpoint its timeline does not hold");
        return;
    };
    match CheckpointRecorded::new(id, entry.kind, entry.thread, entry.turn) {
        Ok(recorded) => events.push(MatterEvent::checkpoint(recorded, commit)),
        Err(error) => tracing::error!(%error, %id, "a decision's checkpoint was not logged"),
    }
}
