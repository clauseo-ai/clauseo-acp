//! The steps of a turn: the prompt's commit (step 1, once `queue.rs` decides the prompt starts
//! now), the matter (step 3: the turn runs beside the matter's other turns, or waits for an
//! undo, a restore or a relocate holding it, `together.rs`), the before-turn checkpoint and the
//! `TurnStarted` commit (step 4), and the end of a turn that never started (a failed launch or
//! checkpoint). The prompt itself (step 5) is in `prompt.rs`; steps 6 to 8 are in `end.rs`.

use std::path::Path;

use clauseo_matters::{CheckpointOutcome, TurnMeta};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandOutcome, ContentBlock, Delivery, ErrorCode,
    MatterHold, MatterId, OutsideEdits, PromptBlock, SteerOutcome, SteerResolved, StopReason,
    StreamId, ThreadId, TurnFailed, TurnId, TurnQueued, TurnStarted,
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
use crate::log_file::Cause;
use crate::matter_worker::{MatterJob, TurnCheckpoint};
use crate::pipeline::MatterEvent;

/// `command.invalid` for an archived thread; `thread.turn_running` while the thread's own undo
/// holds the matter (a turn queued behind its own thread would wait on nothing it can see, and
/// the undo's note must be durable before the next prompt).
pub(super) fn refuse_unsendable(actor: &Actor, thread: ThreadId) -> Result<(), Error> {
    let entry = actor.check_thread(thread)?;
    if entry.row.archived {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "the thread is archived",
        ));
    }
    // The thread holds its matter for its undo, with no current turn.
    let own_undo = entry.turn.current.is_none()
        && actor
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
    let started = actor.now().mono;
    let held = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    held.current = Some(Box::new(Current::new(turn, message, blocks, started)));
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
    if let Ok(matter) = matter_of(actor, thread) {
        end::release_writer(actor, matter, thread);
    }
    actor.release(StreamId::Thread(thread));
}

/// The agent's session is attached: step 3, the matter. The turn runs at once beside the
/// matter's other turns, or waits with `TurnQueued` while an undo, a restore or a relocate
/// holds the matter; never because of another thread's turn.
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
        // Its agent was started again at step 4 (the matter moved): it kept its place.
        if turns.running.contains(&thread) {
            before_turn(actor, thread, turn);
            return Ok(());
        }
        let (holder, hold) = match turns.holder {
            None => {
                super::together::join(actor, matter, thread);
                // An agent-started turn shows from now: it has no prompt that would.
                super::own_turn::log_opened(actor, thread, turn);
                before_turn(actor, thread, turn);
                return Ok(());
            }
            Some(Holder::Thread(holder)) => (holder, MatterHold::Undo),
            // A restore or a relocate belongs to no thread: the turn is logged behind its own
            // thread, the form start-up reconciliation uses too, and its hold says why it waits.
            Some(Holder::Restore) => (thread, MatterHold::Restore),
            Some(Holder::Relocate) => (thread, MatterHold::Relocate),
        };
        turns.queue.push_back(thread);
        if let Some(current) = current(actor, thread) {
            current.phase = TurnPhase::Queued;
        }
        let queued = TurnQueued::new(thread, turn, holder, None)
            .map_err(|error| Error::Internal(error.to_string()))?
            .held_by(hold);
        let mut out = Out::new(actor, thread, None)?;
        out.push(actor, queued)?;
        out.send(actor, Then::Nothing)
    })();
    if let Err(error) = taken {
        tracing::warn!(%thread, error = %Cause(&error), "a turn could not wait for its writer");
    }
}

/// The launch failed: `TurnFailed { agent.* }`, `TurnEnded { Failed, None }`, no
/// `TurnStarted`. During shutdown the turn stays queued for the next start instead.
pub(super) fn on_launch_failed(actor: &mut Actor, thread: ThreadId, turn: TurnId, error: &Error) {
    if current(actor, thread).is_none_or(|current| current.turn != turn) {
        return;
    }
    let code = super::sign_in::code_of(actor, error.code(), &error.to_string(), error.rpc_data());
    let detail = Some(super::close::detail_of(actor, error.detail()));
    // A start that failed on its connection waits for the host's probe (`offline/start.rs`).
    if code == ErrorCode::AgentLaunchFailed
        && !host_stopping(actor)
        && super::offline::start::probe_first(actor, thread, turn, error, detail.clone())
    {
        return;
    }
    end_launch_failed(actor, thread, turn, code, detail);
}

/// Ends `thread`'s unstarted `turn` failed with `code`: `TurnFailed`, `TurnEnded { Failed }`.
/// During shutdown the turn stays queued for the next start instead.
pub(super) fn end_launch_failed(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    code: ErrorCode,
    detail: Option<String>,
) {
    if !host_stopping(actor) {
        let failed = TurnFailed { turn, code, detail };
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
    if let Some(failed) = &failed {
        super::own_turn::refused::unstarted_failed(actor, thread, failed.code);
    }
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
    if agents::restart_if_moved(actor, thread, turn)
        || agents::restart_if_marked(actor, thread, turn)
    {
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
        // The papers the turns running beside it named are not outside edits.
        let running = super::together::named_by_others(actor, matter, thread);
        actor.matter_worker(matter)?.send(
            MatterJob::BeforeTurn { meta, running },
            MatterTag::Turn(job),
        )?;
        actor.work.matter_jobs += 1;
        super::together::before_sent(actor, matter, thread);
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
    // Shutdown's cancel carries its failure (`cancel::shutdown`); the lawyer's carries none.
    let interrupted = if cancelled {
        found.failure.take()
    } else {
        None
    };
    let matter = match matter_of(actor, thread) {
        Ok(matter) => matter,
        Err(error) => {
            tracing::error!(%thread, error = %Cause(&error), "a turn lost its matter");
            end::finish(actor, thread);
            return;
        }
    };
    let checkpoint = match result {
        Ok(mut checkpoint) => {
            // Its papers left out as running turns' are theirs to log (`together.rs`).
            super::together::before_answered(actor, matter, thread, &mut checkpoint.checkpoint);
            checkpoint
        }
        Err(error) => {
            let failed = TurnFailed {
                turn,
                code: error.code(),
                detail: Some(error.detail()),
            };
            let own = super::own_turn::is_own(actor, thread);
            end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
            // An agent-started turn's agent is stopped: it may still be working (a Codex helper,
            // a background command) on output nobody will see. A refused prompt of the lawyer's
            // stops nothing: dropping the held output already ends the loop, and the stop would
            // kill the lawyer's background commands for nothing.
            if own {
                super::own_turn::refused::stop_agent(actor, thread);
            }
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
        let (failed, stop) = match interrupted {
            Some((code, detail)) => (Some(TurnFailed { turn, code, detail }), StopReason::Failed),
            None => (None, StopReason::Cancelled),
        };
        end_unstarted(actor, thread, turn, failed, stop, checkpoint);
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
        tracing::warn!(%thread, error = %Cause(&error), "a turn's start was not logged");
        if failure::blocked(actor, thread) {
            // The unblock ends it.
            if let Some(current) = current(actor, thread) {
                current.store_failed.get_or_insert(error.code());
            }
        } else {
            let failed = TurnFailed {
                turn,
                code: error.code(),
                detail: Some(error.detail()),
            };
            end_unstarted(actor, thread, turn, Some(failed), StopReason::Failed, None);
        }
    }
}

/// The `TurnStarted` commit is durable: step 5, the prompt. A cancel or a failed commit that
/// came meanwhile ends the turn through steps 6 to 8 instead.
pub(super) fn on_started(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let starting =
        |current: &super::Current| current.turn == turn && current.phase == TurnPhase::Starting;
    if !current(actor, thread).is_some_and(|current| starting(current)) {
        return;
    }
    // A turn starts: a run of refused agent-started turns ends (`own_turn/refused.rs`).
    super::own_turn::refused::started(actor, thread);
    let Some(current) = current(actor, thread).filter(|current| starting(current)) else {
        return;
    };
    if current.cancelled {
        end::end_turn(actor, thread, End::cancelled(None));
    } else if let Some(code) = current.store_failed {
        let detail = Some(STORE_DETAIL.to_owned());
        end::end_turn(actor, thread, End::failed(code, detail, None));
    } else if current.own.is_some() {
        super::own_turn::started(actor, thread, turn);
    } else {
        prompt::prompt(actor, thread, turn);
        // Agent output that came between turns comes first in this one (`own_turn.rs`).
        super::own_turn::flush(actor, thread, turn);
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
