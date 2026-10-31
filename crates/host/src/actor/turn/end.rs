//! The end of a turn: the prompt's answer, steps 6 to 8 (the held text and the expiries, the
//! after-turn checkpoint and the commit that ends the turn, the writer passing on), and a host
//! bug that ends a turn early. Store failures are in `failure.rs`.

use clauseo_matters::{CheckpointOutcome, TurnMeta};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, ErrorCode, ExpiryReason, FileChange, FilesChanged,
    MatterId, StopReason, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::input::MatterTag;
use super::agents;
use super::close::{Expiries, TurnClose};
use super::out::Out;
use super::{
    End, JobKind, Then, TurnJob, TurnPhase, asks, cancel, current, failure, host_stopping, known,
    matter_of, state, steps, updates,
};
use crate::error::Error;
use crate::matter_worker::{MatterJob, TurnCheckpoint};

/// The prompt returned: step 6.
pub(super) fn on_prompt_done(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<clauseo_acp::PromptEnd, clauseo_acp::Error>,
) {
    let prompting = current(actor, thread)
        .is_some_and(|current| current.turn == turn && current.phase == TurnPhase::Prompting);
    if !prompting {
        return;
    }
    // Held updates of the turn go first, while it is still live.
    updates::flush_rate(actor, thread);
    let Some(current) = current(actor, thread) else {
        return;
    };
    current.prompt_call = None;
    let end = match result {
        Ok(_) | Err(_) if current.cancelled => End::cancelled(None),
        Ok(answer) => {
            if current.limit.is_none() {
                current.limit = answer.limit.as_ref().and_then(asks::limit_mark);
            }
            End {
                stop: answer.stop,
                failure: None,
                expiry: ExpiryReason::TurnEnded,
                detach: None,
            }
        }
        Err(error) => End::failed(error.code(), Some(error.to_string()), None),
    };
    end_turn(actor, thread, end);
}

/// Steps 6 and 7: the held text and the expiries in one commit, then the after-turn
/// checkpoint, whose answer logs the end.
pub(super) fn end_turn(actor: &mut Actor, thread: ThreadId, mut end: End) {
    let Some(current) = current(actor, thread) else {
        return;
    };
    if current.phase == TurnPhase::Ending {
        return;
    }
    current.phase = TurnPhase::Ending;
    if let Some(call) = current.prompt_call.take() {
        call.abort();
    }
    if let Some(timer) = current.cancel_timer.take() {
        timer.abort();
    }
    if let Some((code, detail)) = current.failure.take()
        && end.failure.is_none()
    {
        end.stop = StopReason::Failed;
        end.failure = Some((code, detail));
    }
    end.store_failed(current.store_failed);
    if end.detach.is_none() {
        end.detach = current.detach.take();
    }
    let (turn, before) = (current.turn, current.checkpoint_before);
    let expiry = end.expiry;
    let paths = std::mem::take(&mut current.tool_paths);
    current.end = Some(end);
    let expired = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        Expiries::Pending(expiry).push_into(actor, &mut out)?;
        out.send(actor, Then::Nothing)
    })();
    if let Err(error) = expired {
        tracing::warn!(%thread, %error, "a turn's expiries were not logged");
    }
    if let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) {
        agent.permissions.clear();
        agent.elicitations.clear();
        agent.url_elicitations.clear();
    }
    let sent = (|| -> Result<(), Error> {
        let matter = matter_of(actor, thread)?;
        let now = actor.now().wall;
        let meta = TurnMeta {
            checkpoint: actor.ids.mint(now)?,
            thread,
            turn,
            at: now,
        };
        let job = TurnJob {
            thread,
            kind: JobKind::After(turn),
        };
        let after = MatterJob::AfterTurn {
            meta,
            tool_paths: paths,
            before,
        };
        actor
            .matter_worker(matter)?
            .send(after, MatterTag::Turn(job))?;
        actor.work.matter_jobs += 1;
        Ok(())
    })();
    if let Err(error) = sent {
        on_after_turn(actor, thread, turn, Err(error));
    }
}

/// The after-turn checkpoint answered: step 7's commit, then step 8 (the snapshot is taken when
/// `TurnEnded` folds; the writer passes on now).
pub(super) fn on_after_turn(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<TurnCheckpoint, Error>,
) {
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.phase == TurnPhase::Ending)
    else {
        return;
    };
    let mut end = current
        .end
        .take()
        .unwrap_or_else(|| End::failed(ErrorCode::Internal, None, None));
    // A commit of steps 6 and 7 may have failed after `end_turn` ran.
    end.store_failed(current.store_failed);
    let limit = current.limit.take();
    // A turn stopped at its limit stays `AtLimit` when only its checkpoint fails.
    let stop = match (&limit, end.stop, &end.failure) {
        (Some(_), StopReason::EndTurn, None) => StopReason::AtLimit,
        (_, stop, _) => stop,
    };
    let mut close = TurnClose::new(turn, stop);
    // Expiries step 6 could not log (its commit failed, the stream rolled back).
    close.expiries = Expiries::Pending(end.expiry);
    close.failure = end.failure;
    close.limit = limit;
    close.detach = end.detach;
    let logged = matter_of(actor, thread).and_then(|matter| {
        match result {
            Ok(done) => after_events(actor, &mut close, matter, thread, done)?,
            Err(error) => {
                close
                    .failure
                    .get_or_insert((error.code(), Some(error.to_string())));
            }
        }
        close.send(actor, thread, Then::Closing(turn))
    });
    failure::closing_sent(actor, thread, &close, logged);
    finish(actor, thread);
}

/// The after-turn checkpoint's part of a close: matter `CheckpointRecorded { AfterTurn }` when
/// R8 created it, `FilesChanged` when anything changed or was skipped, and `checkpoint_after`.
/// A reused id the matter's log does not hold is recorded first, with the changes the lost
/// commit held (the turn's before-turn checkpoint to that id); no tool-call paths are known
/// for them, so each is flagged as possibly holding the lawyer's edits.
pub(super) fn after_events(
    actor: &Actor,
    close: &mut TurnClose,
    matter: MatterId,
    thread: ThreadId,
    done: TurnCheckpoint,
) -> Result<(), Error> {
    let turn = close.turn;
    let unrecorded = known::unrecorded(actor, matter, &done)?;
    let TurnCheckpoint { checkpoint, reused } = done;
    let mut files = checkpoint.changes;
    let id = checkpoint.outcome.id();
    if let CheckpointOutcome::Created(id) = checkpoint.outcome {
        let recorded =
            CheckpointRecorded::new(id, CheckpointKind::AfterTurn, Some(thread), Some(turn))
                .map_err(|error| Error::Internal(error.to_string()))?;
        close.push_matter(matter, recorded);
    }
    if let Some(recorded) = unrecorded {
        close.push_matter(matter, recorded);
        let lost = reused.map(|reused| reused.changes).unwrap_or_default();
        files.extend(lost.into_iter().map(FileChange::with_lawyer_edits));
    }
    if !files.is_empty() || !checkpoint.skipped.is_empty() {
        close.files = Some(FilesChanged {
            turn,
            files,
            skipped: checkpoint.skipped,
        });
    }
    close.checkpoint_after = Some(id);
    Ok(())
}

/// The turn is over: it lets go of the writer, which passes to the next queued turn; during
/// shutdown the agent stops now.
pub(super) fn finish(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.current = None;
    }
    if let Ok(matter) = matter_of(actor, thread) {
        release_writer(actor, matter, thread);
    }
    if host_stopping(actor) {
        agents::stop(actor, thread);
    }
    actor.release(StreamId::Thread(thread));
}

/// `thread` lets go of `matter`'s writer (or its place in the queue); the next queued turn
/// takes it at step 4. Nothing new starts during shutdown.
pub(super) fn release_writer(actor: &mut Actor, matter: MatterId, thread: ThreadId) {
    let shutting = host_stopping(actor);
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    let turns = &mut entry.turns;
    if turns.holder != Some(thread) {
        turns.queue.retain(|queued| *queued != thread);
        return;
    }
    turns.holder = None;
    if shutting {
        actor.release(StreamId::Matter(matter));
        return;
    }
    loop {
        let Some(next) = actor
            .matters
            .get_mut(&matter)
            .and_then(|entry| entry.turns.queue.pop_front())
        else {
            actor.release(StreamId::Matter(matter));
            return;
        };
        let waiting = current(actor, next)
            .filter(|current| current.phase == TurnPhase::Queued)
            .map(|current| current.turn);
        if let Some(turn) = waiting {
            if let Some(entry) = actor.matters.get_mut(&matter) {
                entry.turns.holder = Some(next);
            }
            steps::before_turn(actor, next, turn);
            return;
        }
    }
}

/// A host-side failure of the live turn (a refused text delta): the turn ends `Failed` with
/// it, once the commit being built is sent ([`check_abort`]).
pub(super) fn mark_failed(actor: &mut Actor, thread: ThreadId, error: &Error) {
    if let Some(current) = current(actor, thread) {
        current
            .failure
            .get_or_insert_with(|| (error.code(), Some(error.to_string())));
        current.abort = true;
    }
}

/// Stops the live turn's prompt through the Cancel path when a failure asked for it.
pub(super) fn check_abort(actor: &mut Actor, thread: ThreadId) {
    let Some(current) = current(actor, thread).filter(|current| current.abort) else {
        return;
    };
    current.abort = false;
    if current.live() {
        cancel::cancel_running(actor, thread, None);
    }
}
