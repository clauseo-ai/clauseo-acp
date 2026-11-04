//! `PreviewUndo`: what undoing a step would meet now. The step is read from the thread's log as
//! `UndoStep` reads it; R8's `Matter::edited_since` then compares the step's files in the folder
//! with the after-turn checkpoint by content, on the matter's thread. Takes no writer, writes
//! nothing and logs nothing: a turn may run meanwhile, and the answer is as of the read.

use clauseo_protocol::{Envelope, QueryResult, ThreadId, TurnId};

use super::super::input::{Input, MatterTag, Reply};
use super::super::{Actor, streams};
use super::{LogFor, TurnInput, matter_of, undo};
use crate::error::Error;
use crate::matter_worker::MatterJob;

/// Checks the thread, then reads its log for the step.
pub(crate) fn preview_undo(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    reply: Reply<QueryResult>,
) {
    if let Err(error) = matter_of(actor, thread) {
        let _gone = reply.send(Err(error));
        return;
    }
    let store = actor.store.clone();
    actor.spawn_post(async move {
        let result = streams::read_whole_log(&store, clauseo_protocol::StreamId::Thread(thread))
            .await
            .map(|reopen| reopen.tail);
        Input::Turn(TurnInput::Log {
            thread,
            why: LogFor::PreviewUndo { turn, reply },
            result,
        })
    });
}

/// The log came back: send R8 the step's checkpoints, whose answer goes to `reply`
/// (`matters.rs`).
pub(super) fn on_log(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    reply: Reply<QueryResult>,
    result: Result<Vec<Envelope>, clauseo_store::Error>,
) {
    let job = result
        .map_err(Error::Store)
        .and_then(|log| undo::step_of(&log, turn))
        .and_then(|step| {
            let matter = matter_of(actor, thread)?;
            Ok((matter, (step.before, step.after)))
        });
    let (matter, (before, after)) = match job {
        Ok(job) => job,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    let worker = match actor.matter_worker(matter) {
        Ok(worker) => worker,
        Err(error) => {
            let _gone = reply.send(Err(error));
            return;
        }
    };
    // A refused send drops the tag and with it `reply`: the caller sees the host closed.
    match worker.send(
        MatterJob::EditedSince { before, after },
        MatterTag::Query(reply),
    ) {
        Ok(()) => actor.work.matter_jobs += 1,
        Err(error) => tracing::warn!(%matter, %error, "an undo preview was not sent"),
    }
}
