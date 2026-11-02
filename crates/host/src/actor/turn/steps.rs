//! The steps of a turn: the prompt's commit (step 1, once `queue.rs` decides the prompt starts
//! now), the writer queue (step 3), the
//! before-turn checkpoint and the `TurnStarted` commit (step 4), and the end of a turn that
//! never started (a failed launch or checkpoint). The prompt itself (step 5) is in
//! `prompt.rs`; steps 6 to 8 are in `end.rs`.

use std::path::Path;

use clauseo_matters::{CheckpointOutcome, TurnMeta};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandOutcome, ContentBlock, Delivery, ErrorCode,
    MatterId, OutsideEdits, PromptBlock, SteerOutcome, SteerResolved, StopReason, StreamId,
    ThreadId, TurnFailed, TurnId, TurnQueued, TurnStarted,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::input::MatterTag;
use super::super::{Actor, Tag};
use super::agents::{self, Waiting};
use super::close::TurnClose;
use super::out::Out;
use super::state::STORE_DETAIL;
use super::{
    Current, End, Holder, JobKind, Then, TurnJob, TurnPhase, TurnTag, current, end, failure,
    host_stopping, known, lost, matter_of, prompt, state,
};
use crate::build;
use crate::error::Error;
use crate::file_link;
use crate::matter_worker::{MatterJob, TurnCheckpoint};
use crate::pipeline::MatterEvent;

/// `command.invalid` for an archived thread; `thread.turn_running` while the thread's own undo
/// holds the writer (a turn queued behind its own thread would wait on nothing it can see, and
/// the undo's note must be durable before the next prompt).
pub(super) fn refuse_unsendable(actor: &Actor, thread: ThreadId) -> Result<(), Error> {
    let entry = actor.check_thread(thread)?;
    if entry.row.archived {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "the thread is archived",
        ));
    }
    let own_undo = actor
        .matters
        .get(&entry.row.matter)
        .is_some_and(|matter| matter.turns.holder == Some(Holder::Thread(thread)));
    if own_undo {
        return Err(Error::refused(
            ErrorCode::ThreadTurnRunning,
            "an undo is running in this thread; wait for it",
        ));
    }
    Ok(())
}

/// The prompt's blocks as content: text as it is, a file as a link to it. A file must lie in
/// the thread's matter folder, else `command.invalid`.
pub(super) fn prompt_blocks(
    actor: &Actor,
    thread: ThreadId,
    content: Vec<PromptBlock>,
) -> Result<Vec<ContentBlock>, Error> {
    let folder = Path::new(
        &actor
            .check_matter(matter_of(actor, thread)?)?
            .row
            .folder_path,
    );
    content
        .into_iter()
        .map(|block| match block {
            PromptBlock::Text { text } => Ok(ContentBlock::Text { text }),
            PromptBlock::File { file } => {
                let path = actor.files.get(&file).ok_or_else(|| {
                    Error::refused(ErrorCode::CommandInvalid, format!("no file {file}"))
                })?;
                if !file_link::inside(path, folder) {
                    return Err(Error::refused(
                        ErrorCode::CommandInvalid,
                        format!("file {file} is not in this thread's matter folder"),
                    ));
                }
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok(ContentBlock::ResourceLink {
                    uri: file_link::file_uri(path),
                    name,
                    mime_type: None,
                    title: None,
                    description: None,
                    size: None,
                })
            }
        })
        .collect()
}

/// Step 1 for a prompt that starts now (no current turn, nothing queued ahead of it): one commit
/// of `UserMessage` and the receipt, and for a steer that cannot be real
/// `SteerResolved { SentNow }`; the turn launches once it is durable.
pub(super) fn accept(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    blocks: Vec<ContentBlock>,
    delivery: Delivery,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    let message = actor.ids.mint(now)?;
    let built = build::user_message(turn, message, None, blocks.clone(), delivery);
    let outcome = CommandOutcome::PromptAccepted { turn, message };
    let stream = StreamId::Thread(thread);
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    if delivery == Delivery::Steer {
        let outcome = SteerOutcome::SentNow { turn };
        let resolved = SteerResolved {
            message,
            outcome,
            aside: Vec::new(),
        };
        out.push(actor, resolved)?;
    }
    out.draft.receipt(ctx.receipt(Some(stream), outcome, now));
    let held = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    held.current = Some(Box::new(Current::new(turn, message, blocks)));
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: AfterCommand::Turn(TurnTag {
            thread,
            then: Then::Launch(turn),
        }),
    };
    let sent = out.send_tagged(actor, tag);
    if sent.is_err()
        && let Some(state) = state(actor, thread)
    {
        state.current = None;
    }
    sent
}

/// The `UserMessage` is durable, or a queued turn moved to the head (`queue.rs`): step 2, the
/// agent.
pub(super) fn on_accepted(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let shutting = host_stopping(actor);
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return;
    };
    if current.phase != TurnPhase::Accepting {
        return;
    }
    if shutting {
        // A turn that never started stays queued for the next start.
        forget(actor, thread);
        return;
    }
    current.phase = TurnPhase::Launching;
    agents::ensure(actor, thread, Waiting::Turn(turn));
}

/// The `UserMessage` commit failed: there is no turn.
pub(super) fn on_refused(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if current(actor, thread).is_some_and(|current| current.turn == turn) {
        forget(actor, thread);
    }
}

/// Drops the current turn from memory without logging anything but the detach of an agent
/// that exited while the turn waited.
fn forget(actor: &mut Actor, thread: ThreadId) {
    let dropped = state(actor, thread).and_then(|state| state.current.take());
    if let Some(reason) = dropped.and_then(|current| current.detach) {
        agents::log_detach(actor, thread, reason);
    }
    if let Ok(matter) = matter_of(actor, thread)
        && let Some(entry) = actor.matters.get_mut(&matter)
    {
        entry.turns.queue.retain(|queued| *queued != thread);
    }
    actor.release(StreamId::Thread(thread));
}

/// The agent's session is attached: step 3, the writer.
pub(super) fn on_launched(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let launching = current(actor, thread)
        .is_some_and(|current| current.turn == turn && current.phase == TurnPhase::Launching);
    if !launching || host_stopping(actor) {
        return;
    }
    let taken = (|| -> Result<(), Error> {
        let matter = matter_of(actor, thread)?;
        let turns = &mut actor
            .matters
            .get_mut(&matter)
            .ok_or_else(|| lost("a matter"))?
            .turns;
        let holder = match turns.holder {
            None => {
                turns.holder = Some(Holder::Thread(thread));
                before_turn(actor, thread, turn);
                return Ok(());
            }
            // Its agent was started again at step 4 (the matter moved): it kept the writer.
            Some(Holder::Thread(holder)) if holder == thread => {
                before_turn(actor, thread, turn);
                return Ok(());
            }
            Some(Holder::Thread(holder)) => holder,
            // A restore or a relocate belongs to no thread: the turn is logged behind its own
            // thread, the form start-up reconciliation uses too.
            Some(Holder::Restore | Holder::Relocate) => thread,
        };
        turns.queue.push_back(thread);
        if let Some(current) = current(actor, thread) {
            current.phase = TurnPhase::Queued;
        }
        let queued = TurnQueued::new(thread, turn, holder, None)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let mut out = Out::new(actor, thread, None)?;
        out.push(actor, queued)?;
        out.send(actor, Then::Nothing)
    })();
    if let Err(error) = taken {
        tracing::warn!(%thread, %error, "a turn could not wait for its writer");
    }
}

/// The launch failed: `TurnFailed { agent.* }`, `TurnEnded { Failed, None }`, no
/// `TurnStarted`. During shutdown the turn stays queued for the next start instead.
pub(super) fn on_launch_failed(actor: &mut Actor, thread: ThreadId, turn: TurnId, error: &Error) {
    if current(actor, thread).is_none_or(|current| current.turn != turn) {
        return;
    }
    if !host_stopping(actor) {
        let failed = TurnFailed {
            turn,
            code: error.code(),
            detail: Some(error.to_string()),
        };
        end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
        return;
    }
    forget(actor, thread);
}

/// Ends a turn that never started (no `TurnStarted`) with one commit, and lets it go. The
/// detach of an agent that exited while the turn waited goes last.
pub(super) fn end_unstarted(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    failed: Option<TurnFailed>,
    stop: StopReason,
    checkpoint: Option<(MatterId, TurnCheckpoint)>,
) {
    let mut close = TurnClose::new(turn, stop);
    close.failure = failed.map(|failed| (failed.code, failed.detail));
    close.detach = current(actor, thread).and_then(|current| current.detach.take());
    let sent = (|| -> Result<(), Error> {
        if let Some((matter, checkpoint)) = checkpoint {
            for event in before_events(actor, matter, thread, turn, checkpoint)? {
                close.push_matter(matter, event);
            }
        }
        close.send(actor, thread, Then::Closing(turn))
    })();
    failure::closing_sent(actor, thread, &close, sent);
    end::finish(actor, thread);
}

/// Step 4: the before-turn checkpoint on the matter's thread.
pub(super) fn before_turn(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if agents::restart_if_moved(actor, thread, turn) {
        return;
    }
    if let Some(current) = current(actor, thread) {
        current.phase = TurnPhase::Checkpointing;
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
            kind: JobKind::Before(turn),
        };
        actor
            .matter_worker(matter)?
            .send(MatterJob::BeforeTurn(meta), MatterTag::Turn(job))?;
        actor.work.matter_jobs += 1;
        Ok(())
    })();
    if let Err(error) = sent {
        on_before_turn(actor, thread, turn, Err(error));
    }
}

/// The before-turn checkpoint answered: `TurnStarted` (with the checkpoint's events when it is
/// new), and the prompt once that commit is durable. A cancel or a failed commit that came
/// meanwhile ends the turn instead.
pub(super) fn on_before_turn(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<TurnCheckpoint, Error>,
) {
    let Some(found) = current(actor, thread)
        .filter(|current| current.turn == turn && current.phase == TurnPhase::Checkpointing)
    else {
        return;
    };
    let (cancelled, store_failed) = (found.cancelled, found.store_failed);
    let matter = match matter_of(actor, thread) {
        Ok(matter) => matter,
        Err(error) => {
            tracing::error!(%thread, %error, "a turn lost its matter");
            end::finish(actor, thread);
            return;
        }
    };
    let checkpoint = match result {
        Ok(checkpoint) => checkpoint,
        Err(error) => {
            let failed = TurnFailed {
                turn,
                code: error.code(),
                detail: Some(error.to_string()),
            };
            end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
            return;
        }
    };
    if let Some(code) = store_failed {
        let failed = TurnFailed {
            turn,
            code,
            detail: Some(STORE_DETAIL.to_owned()),
        };
        let checkpoint = Some((matter, checkpoint));
        end_unstarted(
            actor,
            thread,
            turn,
            Some(failed),
            StopReason::Failed,
            checkpoint,
        );
        return;
    }
    if cancelled {
        let checkpoint = Some((matter, checkpoint));
        end_unstarted(actor, thread, turn, None, StopReason::Cancelled, checkpoint);
        return;
    }
    let before = checkpoint.checkpoint.outcome.id();
    if let Some(current) = current(actor, thread) {
        current.phase = TurnPhase::Starting;
        current.checkpoint_before = Some(before);
    }
    let started = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        for event in before_events(actor, matter, thread, turn, checkpoint)? {
            out.push_matter(actor, matter, event)?;
        }
        out.push(
            actor,
            TurnStarted {
                turn,
                checkpoint_before: before,
            },
        )?;
        out.send(actor, Then::Started(turn))
    })();
    if let Err(error) = started {
        tracing::warn!(%thread, %error, "a turn's start was not logged");
        if failure::blocked(actor, thread) {
            // The unblock ends it.
            if let Some(current) = current(actor, thread) {
                current.store_failed.get_or_insert(error.code());
            }
        } else {
            let failed = TurnFailed {
                turn,
                code: error.code(),
                detail: Some(error.to_string()),
            };
            end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
        }
    }
}

/// The `TurnStarted` commit is durable: step 5, the prompt. A cancel or a failed commit that
/// came meanwhile ends the turn through steps 6 to 8 instead.
pub(super) fn on_started(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.phase == TurnPhase::Starting)
    else {
        return;
    };
    if current.cancelled {
        end::end_turn(actor, thread, End::cancelled(None));
    } else if let Some(code) = current.store_failed {
        let detail = Some(STORE_DETAIL.to_owned());
        end::end_turn(actor, thread, End::failed(code, detail, None));
    } else {
        prompt::prompt(actor, thread, turn);
    }
}

/// A before-turn checkpoint's events in the matter's stream: `CheckpointRecorded { BeforeTurn }`
/// and `OutsideEdits` for a new one, the record of a reused id the matter's log does not hold.
fn before_events(
    actor: &Actor,
    matter: MatterId,
    thread: ThreadId,
    turn: TurnId,
    done: TurnCheckpoint,
) -> Result<Vec<MatterEvent>, Error> {
    if let Some(recorded) = known::unrecorded(actor, matter, &done)? {
        return Ok(vec![recorded]);
    }
    let checkpoint = done.checkpoint;
    let CheckpointOutcome::Created(id) = checkpoint.outcome else {
        return Ok(Vec::new());
    };
    let recorded =
        CheckpointRecorded::new(id, CheckpointKind::BeforeTurn, Some(thread), Some(turn))
            .map_err(|error| Error::Internal(error.to_string()))?;
    let mut events = vec![MatterEvent::checkpoint(recorded, checkpoint.commit)];
    if !checkpoint.changes.is_empty() {
        let edits = OutsideEdits {
            checkpoint: id,
            files: checkpoint.changes,
        };
        events.push(MatterEvent::plain(edits));
    }
    Ok(events)
}
