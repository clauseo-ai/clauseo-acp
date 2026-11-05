//! The queue commands: `EditQueuedPrompt`, `RemoveQueuedPrompt`, `SendQueuedNow` and
//! `ResumeQueue`. Each runs once the thread's stream is loaded.

use clauseo_protocol::{
    CommandOutcome, ContentBlock, ErrorCode, StopReason, StreamId, ThreadId, TurnEnded, TurnId,
};

use super::super::super::commands::{AfterCommand, CommandCtx};
use super::super::super::{Actor, Tag};
use super::super::out::Out;
use super::super::{TurnPhase, cancel, lost, state};
use super::{After, command_tag, move_on, queue, sync_held, with_stream};
use crate::build;
use crate::error::Error;

/// A queued turn of the thread, or its current turn while it has not reached its checkpoint
/// (the view still shows it queued): `Some(true)` for the current one.
fn find(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> Result<bool, Error> {
    let state = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    if state.queue.position(turn).is_some() {
        return Ok(false);
    }
    let unstarted = state.current.as_ref().is_some_and(|current| {
        current.turn == turn
            && !current.cancelled
            && matches!(current.phase, TurnPhase::Launching | TurnPhase::Queued)
    });
    if unstarted {
        return Ok(true);
    }
    Err(Error::refused(
        ErrorCode::ThreadTurnNotQueued,
        "that message is no longer queued",
    ))
}

/// `EditQueuedPrompt`: the text replaces the prompt's first text block, its other text blocks
/// go, and its file blocks stay; one commit of `QueuedPromptEdited` and the receipt.
pub(in super::super) fn edit(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
    text: String,
) {
    with_stream(actor, ctx, thread, move |actor| {
        edit_loaded(actor, ctx, thread, turn, &text)
    });
}

fn edit_loaded(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
    text: &str,
) -> Result<(), Error> {
    let current = find(actor, thread, turn)?;
    let state = state(actor, thread).ok_or_else(|| lost("a thread"))?;
    let (message, old) = if current {
        let current = state.current.as_deref().ok_or_else(|| lost("the turn"))?;
        (current.message, current.prompt.clone())
    } else {
        let at = state.queue.position(turn).ok_or_else(|| lost("the turn"))?;
        let queued = state.queue.turns.get(at).ok_or_else(|| lost("the turn"))?;
        (queued.message, queued.content.clone())
    };
    let content = edited(&old, text)?;
    super::super::logout::refuse(actor, thread, &content)?;
    let built = build::queued_prompt_edited(turn, message, content.clone());
    let now = actor.now().wall;
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(actor, built.value)?;
    out.draft.blobs(built.blobs);
    let stream = Some(StreamId::Thread(thread));
    out.draft
        .receipt(ctx.receipt(stream, CommandOutcome::Done, now));
    put_content(actor, thread, turn, content);
    let tag = command_tag(
        ctx,
        CommandOutcome::Done,
        thread,
        After::Edited {
            turn,
            old: old.clone(),
        },
    );
    let sent = out.send_tagged(actor, tag);
    if sent.is_err() {
        put_content(actor, thread, turn, old);
    }
    sent
}

/// The prompt `old` with `text` in place of its text: in the first text block's place (first
/// when it had none), its other text blocks dropped, every other block kept in order.
/// `command.invalid` when nothing would be left to send.
fn edited(old: &[ContentBlock], text: &str) -> Result<Vec<ContentBlock>, Error> {
    let mut content = Vec::with_capacity(old.len() + 1);
    let mut placed = text.is_empty();
    for block in old {
        if matches!(block, ContentBlock::Text { .. }) {
            if !placed {
                content.push(ContentBlock::Text {
                    text: text.to_owned(),
                });
                placed = true;
            }
        } else {
            content.push(block.clone());
        }
    }
    if !placed {
        content.insert(
            0,
            ContentBlock::Text {
                text: text.to_owned(),
            },
        );
    }
    let visible = !text.trim().is_empty();
    if !visible
        && content
            .iter()
            .all(|block| matches!(block, ContentBlock::Text { .. }))
    {
        return Err(Error::refused(
            ErrorCode::CommandInvalid,
            "an edited message without files needs some text",
        ));
    }
    Ok(content)
}

/// Sets the prompt step 5 sends for `turn`, queued or current.
pub(super) fn put_content(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    content: Vec<ContentBlock>,
) {
    let Some(state) = state(actor, thread) else {
        return;
    };
    if let Some(current) = state
        .current
        .as_deref_mut()
        .filter(|current| current.turn == turn)
    {
        current.prompt = content;
    } else if let Some(queued) = state
        .queue
        .turns
        .iter_mut()
        .find(|queued| queued.turn == turn)
    {
        queued.content = content;
    }
}

/// `RemoveQueuedPrompt`: one commit of `TurnEnded { Cancelled, None }` and the receipt. The
/// rest of the queue keeps its state.
pub(in super::super) fn remove(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId, turn: TurnId) {
    with_stream(actor, ctx, thread, move |actor| {
        remove_loaded(actor, ctx, thread, turn)
    });
}

fn remove_loaded(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
) -> Result<(), Error> {
    if find(actor, thread, turn)? {
        if let Some(queue) = queue(actor, thread) {
            queue.removed = Some(turn);
        }
        return cancel::dequeue(actor, Some(ctx), thread, turn);
    }
    let now = actor.now().wall;
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let ended = TurnEnded {
        turn,
        stop: StopReason::Cancelled,
        usage: None,
        checkpoint_after: None,
    };
    out.push(actor, ended)?;
    let stream = Some(StreamId::Thread(thread));
    out.draft
        .receipt(ctx.receipt(stream, CommandOutcome::Done, now));
    // Out of the queue while the commit is in flight, so it cannot start meanwhile; gone once
    // the commit is durable, back in its place if it fails.
    let queue = queue(actor, thread).ok_or_else(|| lost("a thread"))?;
    let at = queue.position(turn).ok_or_else(|| lost("the turn"))?;
    let queued = queue.turns.remove(at).ok_or_else(|| lost("the turn"))?;
    queue.removing.push((at, queued));
    let tag = command_tag(ctx, CommandOutcome::Done, thread, After::Removed(turn));
    let sent = out.send_tagged(actor, tag);
    if sent.is_err() {
        removed(actor, thread, turn, false);
    }
    sent
}

/// The commit removing `turn` returned: durable, the turn is gone (an emptied queue is no
/// longer held); failed, the turn goes back to its place.
pub(super) fn removed(actor: &mut Actor, thread: ThreadId, turn: TurnId, ok: bool) {
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    let Some(index) = queue
        .removing
        .iter()
        .position(|(_, queued)| queued.turn == turn)
    else {
        return;
    };
    let (at, queued) = queue.removing.remove(index);
    if ok {
        if queue.sign_in == Some(turn) {
            // The turn held for the sign-in is gone: the queue stays held the usual way, and the
            // next turn waits for the lawyer's "Send next", not for `restart_agents`.
            queue.sign_in = None;
            tracing::info!(%thread, "the turn held for the sign-in was removed; the queue stays held");
        }
        if queue.is_empty() {
            queue.held = false;
        }
    } else {
        let at = at.min(queue.turns.len());
        queue.turns.insert(at, queued);
    }
    sync_held(actor, thread);
}

fn done_tag(ctx: CommandCtx) -> Tag {
    Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::Nothing,
    }
}

/// `Done` with only the receipt logged.
fn receipt_only(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId) -> Result<(), Error> {
    let now = actor.now().wall;
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let stream = Some(StreamId::Thread(thread));
    out.draft
        .receipt(ctx.receipt(stream, CommandOutcome::Done, now));
    out.send_tagged(actor, done_tag(ctx))
}

/// `SendQueuedNow`: the turn moves to the head and starts once the current turn, cancelled
/// now, has ended; with no current turn it starts now. The move logs nothing but the receipt.
pub(in super::super) fn send_now(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
) {
    with_stream(actor, ctx, thread, move |actor| {
        send_now_loaded(actor, ctx, thread, turn)
    });
}

fn send_now_loaded(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
) -> Result<(), Error> {
    if find(actor, thread, turn)? {
        // Already the turn being started.
        return receipt_only(actor, ctx, thread);
    }
    receipt_only(actor, ctx, thread)?;
    if let Some(queue) = queue(actor, thread)
        && let Some(at) = queue.position(turn)
        && let Some(mut queued) = queue.turns.remove(at)
    {
        queued.forced = true;
        queue.turns.push_front(queued);
        queue.held = false;
    }
    cancel::cancel_current(actor, thread);
    move_on(actor, thread);
    sync_held(actor, thread);
    Ok(())
}

/// `ResumeQueue`: a held queue's head starts at step 2; nothing else happens when the queue is
/// empty or not held. Only the receipt is logged.
pub(in super::super) fn resume(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId) {
    with_stream(actor, ctx, thread, move |actor| {
        resume_loaded(actor, ctx, thread)
    });
}

fn resume_loaded(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId) -> Result<(), Error> {
    receipt_only(actor, ctx, thread)?;
    if let Some(queue) = queue(actor, thread) {
        queue.held = false;
    }
    move_on(actor, thread);
    sync_held(actor, thread);
    Ok(())
}
