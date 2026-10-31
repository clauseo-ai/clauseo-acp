//! The steps of a turn: the prompt's commit (step 1), the writer queue (step 3), the
//! before-turn checkpoint (step 4), the prompt (step 5), and the end (steps 6 to 8), plus the
//! failures that cut a turn short: the launch, a host bug, a store failure.

use clauseo_matters::{Checkpoint, CheckpointOutcome, TurnMeta};
use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, CommandOutcome, ContentBlock, Delivery, ErrorCode,
    MatterId, OutsideEdits, PromptBlock, StopReason, StreamId, ThreadId, TurnEnded, TurnFailed,
    TurnId, TurnQueued, TurnStarted, ViewEntry,
};
use futures::future::{AbortHandle, Abortable};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::input::{Input, MatterTag};
use super::super::{Actor, Tag};
use super::agents::{self, Stage, Waiting};
use super::out::Out;
use super::{
    AgentReply, Current, End, JobKind, Phase, Then, TurnInput, TurnJob, TurnTag, current, end,
    lost, matter_of, spawn, state, stopping,
};
use crate::build;
use crate::error::Error;
use crate::matter_worker::MatterJob;
use crate::undo_note::{self, NoteInput};

/// Step 1: `SendPrompt` on a thread with no current turn logs `UserMessage` with the receipt.
pub(super) fn send_prompt(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    content: Vec<PromptBlock>,
    delivery: Delivery,
) -> Result<(), Error> {
    refuse_busy(actor, thread)?;
    let blocks = prompt_blocks(actor, content)?;
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let done = loaded.and_then(|()| accept(actor, ctx, thread, blocks, delivery));
            if let Err(error) = done {
                actor.finish_command(ctx.id, Err(error));
            }
            actor.release(stream);
        }),
    );
    Ok(())
}

/// `command.invalid` for an archived thread, `thread.turn_running` while a turn is current.
fn refuse_busy(actor: &Actor, thread: ThreadId) -> Result<(), Error> {
    let entry = actor.check_thread(thread)?;
    if entry.row.archived {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "the thread is archived",
        ));
    }
    if entry.turn.current.is_some() {
        return Err(Error::refused(
            ErrorCode::ThreadTurnRunning,
            "a turn is running in this thread; wait for it or cancel it",
        ));
    }
    Ok(())
}

/// The prompt's blocks as content: text as it is, a file as a link to it.
fn prompt_blocks(actor: &Actor, content: Vec<PromptBlock>) -> Result<Vec<ContentBlock>, Error> {
    content
        .into_iter()
        .map(|block| match block {
            PromptBlock::Text { text } => Ok(ContentBlock::Text { text }),
            PromptBlock::File { file } => {
                let path = actor.files.get(&file).ok_or_else(|| {
                    Error::refused(ErrorCode::CommandInvalid, format!("no file {file}"))
                })?;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                Ok(ContentBlock::ResourceLink {
                    uri: format!("file://{}", path.display()),
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

fn accept(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    blocks: Vec<ContentBlock>,
    delivery: Delivery,
) -> Result<(), Error> {
    refuse_busy(actor, thread)?;
    let now = actor.now().wall;
    let turn: TurnId = actor.ids.mint(now)?;
    let message = actor.ids.mint(now)?;
    let built = build::user_message(turn, message, None, blocks.clone(), delivery);
    let outcome = CommandOutcome::PromptAccepted { turn, message };
    let stream = StreamId::Thread(thread);
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    out.draft.receipt(ctx.receipt(Some(stream), outcome, now));
    let held = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    held.current = Some(Box::new(Current::new(turn, blocks)));
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

/// The `UserMessage` is durable: step 2, the agent.
pub(super) fn on_accepted(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let shutting = stopping(actor);
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return;
    };
    if current.phase != Phase::Accepting {
        return;
    }
    if shutting {
        // A turn that never started stays queued for the next start.
        forget(actor, thread);
        return;
    }
    current.phase = Phase::Launching;
    agents::ensure(actor, thread, Waiting::Turn(turn));
}

/// The `UserMessage` commit failed: there is no turn.
pub(super) fn on_refused(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if current(actor, thread).is_some_and(|current| current.turn == turn) {
        forget(actor, thread);
    }
}

/// Drops the current turn from memory without logging anything.
fn forget(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.current = None;
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
        .is_some_and(|current| current.turn == turn && current.phase == Phase::Launching);
    if !launching || stopping(actor) {
        return;
    }
    let taken = (|| -> Result<(), Error> {
        let matter = matter_of(actor, thread)?;
        let turns = &mut actor
            .matters
            .get_mut(&matter)
            .ok_or_else(|| lost("a matter"))?
            .turns;
        let Some(holder) = turns.holder else {
            turns.holder = Some(thread);
            before_turn(actor, thread, turn);
            return Ok(());
        };
        turns.queue.push_back(thread);
        if let Some(current) = current(actor, thread) {
            current.phase = Phase::Queued;
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
    if !stopping(actor) {
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

/// Ends a turn that never started (no `TurnStarted`) with one commit, and lets it go.
pub(super) fn end_unstarted(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    failed: Option<TurnFailed>,
    stop: StopReason,
    checkpoint: Option<(MatterId, Checkpoint)>,
) {
    let sent = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        if let Some((matter, checkpoint)) = checkpoint {
            before_events(actor, &mut out, matter, thread, turn, checkpoint)?;
        }
        if let Some(failed) = failed {
            out.push(actor, failed)?;
        }
        out.push(
            actor,
            TurnEnded {
                turn,
                stop,
                usage: None,
                checkpoint_after: None,
            },
        )?;
        out.send(actor, Then::Nothing)
    })();
    if let Err(error) = sent {
        tracing::warn!(%thread, %error, "the end of a turn was not logged");
    }
    end::finish(actor, thread);
}

/// Step 4: the before-turn checkpoint on the matter's thread.
pub(super) fn before_turn(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(current) = current(actor, thread) {
        current.phase = Phase::Checkpointing;
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
/// new), then the prompt. A cancel that came meanwhile ends the turn instead.
pub(super) fn on_before_turn(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    result: Result<Checkpoint, Error>,
) {
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.phase == Phase::Checkpointing)
    else {
        return;
    };
    let cancelled = current.cancelled;
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
    if cancelled {
        let checkpoint = Some((matter, checkpoint));
        end_unstarted(actor, thread, turn, None, StopReason::Cancelled, checkpoint);
        return;
    }
    let started = (|| -> Result<(), Error> {
        let mut out = Out::new(actor, thread, None)?;
        let before = checkpoint.outcome.id();
        before_events(actor, &mut out, matter, thread, turn, checkpoint)?;
        out.push(
            actor,
            TurnStarted {
                turn,
                checkpoint_before: before,
            },
        )?;
        out.send(actor, Then::Nothing)
    })();
    match started {
        Ok(()) => prompt(actor, thread, turn),
        Err(error) => {
            tracing::warn!(%thread, %error, "a turn's start was not logged");
            end::finish(actor, thread);
        }
    }
}

/// A new before-turn checkpoint's events in the matter's stream.
fn before_events(
    actor: &mut Actor,
    out: &mut Out,
    matter: MatterId,
    thread: ThreadId,
    turn: TurnId,
    checkpoint: Checkpoint,
) -> Result<(), Error> {
    let CheckpointOutcome::Created(id) = checkpoint.outcome else {
        return Ok(());
    };
    let recorded =
        CheckpointRecorded::new(id, CheckpointKind::BeforeTurn, Some(thread), Some(turn))
            .map_err(|error| Error::Internal(error.to_string()))?;
    out.push_matter(actor, matter, recorded)?;
    if !checkpoint.changes.is_empty() {
        let edits = OutsideEdits {
            checkpoint: id,
            files: checkpoint.changes,
        };
        out.push_matter(actor, matter, edits)?;
    }
    Ok(())
}

/// Step 5: the prompt, with the undo note first.
fn prompt(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let note = take_note(actor, thread);
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .filter(|agent| agent.stage == Stage::Attached)
        .and_then(|agent| {
            let session = agent.session.clone()?;
            Some((
                agent.id,
                agent.link.client.clone(),
                agent.link.fence(),
                session,
            ))
        });
    let Some((id, client, fence, session)) = agent else {
        if let Some(current) = current(actor, thread) {
            current.phase = Phase::Prompting;
        }
        let end = End::failed(
            ErrorCode::AgentExited,
            Some("the agent is gone".to_owned()),
            None,
        );
        end::end_turn(actor, thread, end);
        return;
    };
    let Some(current) = current(actor, thread) else {
        return;
    };
    current.phase = Phase::Prompting;
    let blocks = std::mem::take(&mut current.prompt);
    let mut content = Vec::with_capacity(blocks.len() + 1);
    if let Some(text) = note {
        content.push(ContentBlock::Text { text });
    }
    content.extend(blocks);
    let (abort, registration) = AbortHandle::new_pair();
    current.prompt_call = Some(abort);
    let call = Abortable::new(
        async move {
            let result = client.prompt(&session, turn, content).await;
            fence.await;
            result
        },
        registration,
    );
    let post = actor.post.clone();
    spawn(actor, async move {
        // Aborted: the cancel fallback dropped the prompt; nothing to post.
        if let Ok(result) = call.await {
            let reply = AgentReply::Prompt { turn, result };
            let input = TurnInput::Agent {
                thread,
                agent: id,
                reply,
            };
            let _gone = post.try_send(Input::Turn(input));
        }
    });
}

/// The undo note's text for the next prompt, and the note cleared for the turn starting now.
fn take_note(actor: &mut Actor, thread: ThreadId) -> Option<String> {
    ensure_note(actor, thread);
    let paths = state(actor, thread)?.note.replace(Vec::new())?;
    undo_note::note_text(&paths)
}

/// Reads the note from the loaded state the first time it is needed: the files undone since
/// the last turn's step.
pub(super) fn ensure_note(actor: &mut Actor, thread: ThreadId) {
    if state(actor, thread).is_none_or(|state| state.note.is_some()) {
        return;
    }
    let paths = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .map(|state| {
            let inputs = state.entries().values().filter_map(|entry| {
                if matches!(entry, ViewEntry::Step(_)) {
                    Some(NoteInput::TurnStarted)
                } else if let ViewEntry::Marker(marker) = entry
                    && let clauseo_protocol::MarkerKind::StepUndone { restored, .. } = &marker.kind
                {
                    Some(NoteInput::Undone { restored })
                } else {
                    None
                }
            });
            undo_note::paths_since_last_turn(inputs)
        })
        .unwrap_or_default();
    if let Some(state) = state(actor, thread) {
        state.note = Some(paths);
    }
}
