//! A restore's first job, before any file is written: R8 records the folder as it is (a new
//! `outside-edits` checkpoint, or the last checkpoint the folder equals) and writes nothing; one
//! commit of the matter's stream logs that checkpoint, and the restore itself is sent only once
//! that commit is durable. R8's write-back is not atomic: a crash part way through it would
//! otherwise leave the copy it replaces in the history alone, where Versions, which reads the
//! matter's stream, never shows it. A reused id the matter's log does not hold (an earlier
//! restore's checkpoint whose log never landed) is recorded here too, with its row. The restore
//! that follows has fresh ids: its own outside-edits checkpoint is new only for what changed in
//! between, logged with the restore as before, so no checkpoint is logged twice.

use clauseo_matters::CheckpointOutcome;
use clauseo_protocol::{CheckpointRecorded, MatterId};

use super::super::super::input::MatterTag;
use super::super::super::{Actor, Tag};
use super::super::{known, undo};
use super::{RestoreAsk, give_up, job, send_restore};
use crate::error::Error;
use crate::matter_worker::{MatterJob, RecordDone};
use crate::pipeline::{Draft, MatterEvent};

/// Sends the restore's first job to the matter's thread.
pub(super) fn send(actor: &mut Actor, ask: RestoreAsk) -> Result<(), Error> {
    let job = job(actor, &ask)?;
    let matter = ask.matter;
    actor.matter_worker(matter)?.send(
        MatterJob::RestoreRecord(job),
        MatterTag::Restore(Box::new(ask)),
    )?;
    actor.work.matter_jobs += 1;
    Ok(())
}

/// R8 answered the first job: its checkpoint is logged when it is new, or reused and not in the
/// matter's log, and the restore is sent once that commit is durable; at once when there is
/// nothing to log. A refusal gives up as the restore's own would: nothing is written.
pub(crate) fn on_recorded(
    actor: &mut Actor,
    ask: Box<RestoreAsk>,
    result: Result<RecordDone, Error>,
) {
    let (ctx, matter) = (ask.ctx, ask.matter);
    let sent = result
        .and_then(|done| events(actor, matter, done))
        .and_then(|to_log| {
            if to_log.is_empty() {
                send_restore(actor, *ask)
            } else {
                log(actor, ask, to_log)
            }
        });
    if let Err(error) = sent {
        give_up(actor, matter, ctx, error);
    }
}

/// The events logging the first job's checkpoint: for a new one, its `CheckpointRecorded`
/// (with its row) and `OutsideEdits`; for a reused id the matter's log does not hold, its
/// `CheckpointRecorded` with its row, as R8's timeline has it; none otherwise.
fn events(actor: &Actor, matter: MatterId, done: RecordDone) -> Result<Vec<MatterEvent>, Error> {
    let RecordDone { checkpoint, reused } = done;
    let id = match checkpoint.outcome {
        CheckpointOutcome::Created(_) => return undo::outside_edits_events(Some(checkpoint)),
        CheckpointOutcome::Reused(id) if known::recorded(actor, matter, id) => {
            return Ok(Vec::new());
        }
        CheckpointOutcome::Reused(id) => id,
    };
    let Some(entry) = reused else {
        tracing::error!(%id, "R8 reused a checkpoint its timeline does not hold");
        return Ok(Vec::new());
    };
    let recorded = CheckpointRecorded::new(id, entry.kind, entry.thread, entry.turn)
        .map_err(|error| Error::Internal(error.to_string()))?;
    Ok(vec![MatterEvent::checkpoint(recorded, checkpoint.commit)])
}

/// One commit of the matter's stream with `events` and no receipt: the command is answered
/// when the restore is logged.
fn log(actor: &mut Actor, ask: Box<RestoreAsk>, events: Vec<MatterEvent>) -> Result<(), Error> {
    let (command, matter) = (ask.ctx.id, ask.matter);
    let now = actor.now().wall;
    let mut draft = Draft::new();
    for event in events {
        actor.pipeline.push_matter(
            &mut draft,
            matter,
            event,
            Some(command),
            now,
            &mut actor.ids,
        )?;
    }
    actor
        .send_commit(draft, Tag::RestoreRecord(ask))
        .map(|_ticket| ())
}

/// The commit logging the first job's checkpoint returned: durable, the restore is sent;
/// failed, the restore gives up with nothing written, and a retry records that checkpoint as
/// one reused.
pub(crate) fn logged(actor: &mut Actor, ask: RestoreAsk, outcome: Result<(), Error>) {
    let (ctx, matter) = (ask.ctx, ask.matter);
    if let Err(error) = outcome.and_then(|()| send_restore(actor, ask)) {
        give_up(actor, matter, ctx, error);
    }
}
