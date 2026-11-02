//! The end of a turn: the prompt's answer, steps 6 to 8 (the held text and the expiries, the
//! after-turn checkpoint and the commit that ends the turn, the writer passing on), and a host
//! bug that ends a turn early. Store failures are in `failure.rs`.
//!
//! A turn that stops at the limit tells `limits` (`TurnEnded { read_now: true }`). With no reset
//! time and an agent the model reads, its `TurnStoppedAtLimit` is left out of the end commit and
//! logged after the read (`limits::stop_later`); an agent id with no kind gets no read, so its
//! stop rides in the end commit as before.
//!
//! The Codex fallback: a Codex prompt that fails with a JSON-RPC error may be codex-acp's form
//! of the limit, which Clauseo never parses from prose. Step 6 runs as for any failure, then
//! asks for one Codex read; step 7's commit waits until that read is done, or 10 s on the host
//! clock (the read may wait behind a question of another Codex thread or a read in flight),
//! then ends the turn `AtLimit` with its `TurnStoppedAtLimit` when `Limits::at_limit` answers,
//! else `TurnFailed` as before. The turn stays `Ending` meanwhile, so a cancel, an agent's exit
//! (its detach joins the end) or a commit failure changes nothing but what that commit holds,
//! and shutdown decides at once.

use clauseo_matters::{CheckpointOutcome, TurnMeta};
use clauseo_protocol::{
    AgentKind, CheckpointKind, CheckpointRecorded, ErrorCode, ExpiryReason, FileChange,
    FilesChanged, MatterId, StopReason, StreamId, ThreadId, TurnId,
};
use futures::future::AbortHandle;

use super::super::input::MatterTag;
use super::super::{Actor, limits};
use super::agents;
use super::close::{Expiries, TurnClose};
use super::out::Out;
use super::{
    End, Holder, JobKind, LIMIT_READ_WAIT, LimitMark, Then, TimerKind, TurnJob, TurnPhase,
    TurnTimer, asks, cancel, current, failure, host_stopping, known, matter_of, state, steps,
    updates,
};
use crate::error::Error;
use crate::matter_worker::{MatterJob, TurnCheckpoint};
use crate::pipeline::MatterEvent;

/// A Codex turn whose end waits for a limits read (the module doc's Codex fallback).
#[derive(Debug, Default)]
pub(super) struct LimitRead {
    /// The read is done, or the wait is over: the end goes on.
    done: bool,
    /// The 10 s bound, once armed.
    timer: Option<AbortHandle>,
    /// The after-turn checkpoint's answer, held until the read is done.
    held: Option<Result<TurnCheckpoint, Error>>,
}

#[cfg(test)]
impl LimitRead {
    /// Whether it holds the after-turn checkpoint's answer.
    pub(super) const fn holds_checkpoint(&self) -> bool {
        self.held.is_some()
    }
}

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
    // A turn whose commit already failed ends `Failed` whatever the read says.
    let store_failed = current(actor, thread).is_some_and(|current| current.store_failed.is_some());
    let fallback = matches!(result, Err(clauseo_acp::Error::Rpc { .. }))
        && !host_stopping(actor)
        && !store_failed
        && limits::reads_for(actor, thread) == Some(AgentKind::Codex);
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
        Err(error) => {
            if fallback {
                current.limit_read = Some(LimitRead::default());
            }
            End::failed(error.code(), Some(error.to_string()), None)
        }
    };
    end_turn(actor, thread, end);
}

/// A Codex prompt's `.codex` re-check refused it, so nothing reached the agent: the agent is
/// stopped (its app-server would load the folder's settings on any restart) and the turn ends
/// failed with the refusal's code.
pub(super) fn on_prompt_refused(actor: &mut Actor, thread: ThreadId, turn: TurnId, error: &Error) {
    let prompting = current(actor, thread)
        .is_some_and(|current| current.turn == turn && current.phase == TurnPhase::Prompting);
    if !prompting {
        return;
    }
    tracing::warn!(%thread, %error, "a Codex prompt was refused before it was sent");
    if let Some(current) = current(actor, thread) {
        current.prompt_call = None;
    }
    let cancelled = current(actor, thread).is_some_and(|current| current.cancelled);
    let end = if cancelled {
        End::cancelled(None)
    } else {
        End::failed(error.code(), Some(error.to_string()), None)
    };
    end_turn(actor, thread, end);
    agents::stop(actor, thread);
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
    limits::asking_changed(actor);
    let reads = state(actor, thread)
        .and_then(|state| state.current.as_deref())
        .is_some_and(|current| current.limit_read.is_some());
    if reads {
        // Once, after the turn's own questions are gone, so none of them holds the read back.
        limits::turn_ended(actor, AgentKind::Codex, true);
        let timer = TurnTimer {
            thread,
            kind: TimerKind::LimitRead(turn),
        };
        let timer = actor.arm_timer(LIMIT_READ_WAIT, timer);
        let waiting = state(actor, thread)
            .and_then(|state| state.current.as_deref_mut())
            .and_then(|current| current.limit_read.as_mut());
        match waiting {
            Some(read) => read.timer = Some(timer),
            None => timer.abort(),
        }
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
    if let Some(read) = current.limit_read.as_mut().filter(|read| !read.done) {
        read.held = Some(result);
        return;
    }
    let fallback = current.limit_read.take().is_some();
    let mut end = current
        .end
        .take()
        .unwrap_or_else(|| End::failed(ErrorCode::Internal, None, None));
    // A commit of steps 6 and 7 may have failed after `end_turn` ran.
    let store_failed = current.store_failed;
    end.store_failed(store_failed);
    let mut limit = current.limit.take();
    if fallback && store_failed.is_none() {
        // The read decides: at the limit, the prompt's failure was the limit.
        if let Some(hit) = limits::at_limit(actor, AgentKind::Codex) {
            end.stop = StopReason::EndTurn;
            end.failure = None;
            limit = Some(LimitMark {
                window: hit.window,
                resets_at: hit.resets_at,
                reason: hit.reason,
            });
        }
    }
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
    if !fallback {
        // The fallback asked for its read before this commit, and asks nothing after it.
        tell_limits(actor, thread, &mut close);
    }
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

/// `TurnEnded` for `limits`, with `read_now` for a stop at the limit. A stop with no reset time
/// whose agent the model reads leaves its `TurnStoppedAtLimit` to that read.
fn tell_limits(actor: &mut Actor, thread: ThreadId, close: &mut TurnClose) {
    if close.stop != StopReason::AtLimit {
        limits::turn_ended_in(actor, thread, false);
        return;
    }
    let later = close
        .limit
        .as_ref()
        .filter(|limit| limit.resets_at.is_none())
        .map(|limit| limit.reason)
        .zip(limits::reads_for(actor, thread));
    match later {
        Some((reason, agent)) => {
            close.limit = None;
            limits::stop_later(actor, agent, thread, close.turn, reason);
        }
        None => limits::turn_ended_in(actor, thread, true),
    }
}

/// A limits read of `agent` is done: each Codex turn waiting on one goes on to its end.
pub(crate) fn limits_read(actor: &mut Actor, agent: AgentKind) {
    if agent == AgentKind::Codex {
        stop_waiting_for_reads(actor);
    }
}

/// The 10 s wait for `turn`'s limits read is over.
pub(super) fn on_limit_read_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    read_over(actor, thread, Some(turn));
}

/// Every turn waiting for a limits read goes on: a Codex read is done, or the host shuts down.
pub(super) fn stop_waiting_for_reads(actor: &mut Actor) {
    let waiting: Vec<ThreadId> = actor
        .threads
        .iter()
        .filter(|(_, entry)| {
            let current = entry.turn.current.as_deref();
            current.is_some_and(|current| current.limit_read.is_some())
        })
        .map(|(thread, _)| *thread)
        .collect();
    for thread in waiting {
        read_over(actor, thread, None);
    }
}

/// The wait of `thread`'s turn (of `turn`, when given) for its limits read is over: the end
/// goes on now if the after-turn checkpoint has answered, else when it does.
fn read_over(actor: &mut Actor, thread: ThreadId, turn: Option<TurnId>) {
    let Some(current) =
        current(actor, thread).filter(|current| turn.is_none_or(|turn| turn == current.turn))
    else {
        return;
    };
    let turn = current.turn;
    let Some(read) = current.limit_read.as_mut().filter(|read| !read.done) else {
        return;
    };
    read.done = true;
    if let Some(timer) = read.timer.take() {
        timer.abort();
    }
    if let Some(result) = read.held.take() {
        on_after_turn(actor, thread, turn, result);
    }
}

/// The after-turn checkpoint's part of a close: matter `CheckpointRecorded { AfterTurn }` when
/// R8 created it, `FilesChanged` when anything changed or was skipped, and `checkpoint_after`.
/// A reused id the matter's log does not hold is recorded first, with the changes the lost
/// commit held (the turn's before-turn checkpoint to that id); no tool-call paths are known
/// for them, so each is flagged as possibly holding the lawyer's edits. The start-up repair's
/// own checkpoint logs those changes even when the id is already recorded.
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
        close.push_matter(matter, MatterEvent::checkpoint(recorded, checkpoint.commit));
    }
    let own = reused.as_ref().is_some_and(|reused| reused.own);
    let lost = unrecorded.is_some() || own;
    if let Some(recorded) = unrecorded {
        close.push_matter(matter, recorded);
    }
    if lost {
        let changes = reused.map(|reused| reused.changes).unwrap_or_default();
        files.extend(changes.into_iter().map(FileChange::with_lawyer_edits));
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
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    let turns = &mut entry.turns;
    if turns.holder != Some(Holder::Thread(thread)) {
        turns.queue.retain(|queued| *queued != thread);
        return;
    }
    pass_writer(actor, matter);
}

/// The holder of `matter`'s writer has let go: the next queued turn takes it, or nothing
/// does during shutdown.
pub(super) fn pass_writer(actor: &mut Actor, matter: MatterId) {
    let shutting = host_stopping(actor);
    let Some(entry) = actor.matters.get_mut(&matter) else {
        return;
    };
    entry.turns.holder = None;
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
                entry.turns.holder = Some(Holder::Thread(next));
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
