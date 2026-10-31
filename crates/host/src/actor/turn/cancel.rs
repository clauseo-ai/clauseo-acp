//! Cancel: a turn that has not started is dequeued with `TurnEnded { Cancelled, None }`; a
//! running one gets one commit closing its pending requests and unfinished tool calls, then
//! `session/cancel` and the 5 s fallback, after which the agent is stopped. Shutdown cancels
//! every running turn the same way and stops every agent.

use clauseo_protocol::{
    CommandOutcome, ElicitationOutcome, ElicitationResolved, ElicitationResponse, PermissionChoice,
    PermissionResolved, Principal, StopReason, StreamId, ThreadId, ToolCallPatch, ToolCallStatus,
    ToolCallUpdated, TurnEnded, TurnId,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::{Actor, Tag};
use super::agents::{self, Stage};
use super::out::Out;
use super::{
    CANCEL_WAIT, End, Phase, Then, TimerKind, TurnTimer, current, end, matter_of, spawn, state,
    updates,
};
use crate::error::Error;

/// `Cancel { thread }`: stops the thread's current turn only.
pub(super) fn cancel_command(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
) -> Result<(), Error> {
    let found =
        current(actor, thread).map(|current| (current.phase, current.cancelled, current.turn));
    match found {
        Some((Phase::Accepting | Phase::Launching | Phase::Queued, false, turn)) => {
            dequeue(actor, ctx, thread, turn)
        }
        Some((Phase::Prompting, false, _)) => {
            cancel_running(actor, thread, Some(ctx));
            Ok(())
        }
        Some((Phase::Checkpointing, false, _)) => {
            // Ended when the checkpoint answers, before anything is sent.
            if let Some(current) = current(actor, thread) {
                current.cancelled = true;
            }
            receipt_only(actor, ctx, thread)
        }
        Some((_, true, _) | (Phase::Stopping | Phase::Ending, false, _)) | None => {
            receipt_only(actor, ctx, thread)
        }
    }
}

/// `Done` with nothing logged but the receipt.
fn receipt_only(actor: &mut Actor, ctx: CommandCtx, thread: ThreadId) -> Result<(), Error> {
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let now = actor.now().wall;
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), CommandOutcome::Done, now));
    out.send_tagged(actor, done_tag(ctx))
}

fn done_tag(ctx: CommandCtx) -> Tag {
    Tag::Command {
        id: ctx.id,
        outcome: CommandOutcome::Done,
        after: AfterCommand::Nothing,
    }
}

/// A turn that has not started: `TurnEnded { Cancelled, None }`, no `TurnStarted`.
fn dequeue(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    turn: TurnId,
) -> Result<(), Error> {
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    out.push(
        actor,
        TurnEnded {
            turn,
            stop: StopReason::Cancelled,
            usage: None,
            checkpoint_after: None,
        },
    )?;
    let now = actor.now().wall;
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), CommandOutcome::Done, now));
    out.send_tagged(actor, done_tag(ctx))?;
    if let Some(state) = state(actor, thread) {
        state.current = None;
    }
    if let Ok(matter) = matter_of(actor, thread) {
        end::release_writer(actor, matter, thread);
    }
    actor.release(StreamId::Thread(thread));
    Ok(())
}

/// The running turn: one commit of `PermissionResolved { Cancelled }` per pending request,
/// `ElicitationResolved { Cancelled }` per pending elicitation and `Cancelled` for each
/// unfinished tool call (with the command's receipt when a command asked), then
/// `session/cancel`, the elicitations answered `Cancel`, and the fallback timer.
pub(super) fn cancel_running(actor: &mut Actor, thread: ThreadId, ctx: Option<CommandCtx>) {
    // Updates the rate cap holds came before the cancel.
    updates::flush_rate(actor, thread);
    let Some(running) = current(actor, thread) else {
        return;
    };
    running.cancelled = true;
    let turn = running.turn;
    let logged = log_cancel(actor, thread, turn, ctx);
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "a cancel was not logged");
        if let Some(ctx) = ctx {
            actor.finish_command(ctx.id, Err(error));
        }
    }
    if let Some(agent) = state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .filter(|agent| agent.stage == Stage::Attached)
    {
        agent.permissions.clear();
        agent.url_elicitations.clear();
        let keys: Vec<_> = std::mem::take(&mut agent.elicitations)
            .into_values()
            .collect();
        let client = agent.link.client.clone();
        if let Some(session) = agent.session.clone() {
            spawn(actor, async move {
                if let Err(error) = client.cancel(&session).await {
                    tracing::debug!(%error, "session/cancel failed");
                }
                for key in keys {
                    if let Err(error) = client
                        .answer_elicitation(key, ElicitationResponse::Cancel)
                        .await
                    {
                        tracing::debug!(%error, "an elicitation was not answered cancelled");
                    }
                }
            });
        }
    }
    let timer = actor.arm_timer(
        CANCEL_WAIT,
        TurnTimer {
            thread,
            kind: TimerKind::Cancel(turn),
        },
    );
    if let Some(current) = current(actor, thread) {
        current.cancel_timer = Some(timer);
    }
}

fn log_cancel(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    ctx: Option<CommandCtx>,
) -> Result<(), Error> {
    let (permissions, elicitations, tools): (Vec<_>, Vec<_>, Vec<_>) = actor
        .loaded(StreamId::Thread(thread))
        .map(|loaded| {
            (
                loaded.sent.permissions().map(|(id, _)| id).collect(),
                loaded.sent.elicitations().map(|(id, _)| id).collect(),
                loaded.sent.unfinished_tools(turn).collect(),
            )
        })
        .unwrap_or_default();
    let mut out = Out::new(actor, thread, ctx.map(|ctx| ctx.id))?;
    for request in permissions {
        let resolved = PermissionResolved {
            request,
            choice: PermissionChoice::Cancelled,
            by: Principal::LocalDesktop,
        };
        out.push(actor, resolved)?;
    }
    for elicitation in elicitations {
        let resolved = ElicitationResolved {
            elicitation,
            outcome: ElicitationOutcome::Cancelled,
            by: Principal::LocalDesktop,
        };
        out.push(actor, resolved)?;
    }
    for tool_call in tools {
        let patch = ToolCallPatch {
            status: Some(ToolCallStatus::Cancelled),
            ..ToolCallPatch::default()
        };
        out.push(actor, ToolCallUpdated { tool_call, patch })?;
    }
    match ctx {
        Some(ctx) => {
            let now = actor.now().wall;
            out.draft.receipt(ctx.receipt(
                Some(StreamId::Thread(thread)),
                CommandOutcome::Done,
                now,
            ));
            out.send_tagged(actor, done_tag(ctx))
        }
        None => out.send(actor, Then::Nothing),
    }
}

/// The cancel fallback fired before the agent answered: drop the prompt, stop the agent; the
/// turn ends once it is gone (`agents::on_ended`).
pub(super) fn on_cancel_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(current) = current(actor, thread).filter(|current| {
        current.turn == turn && current.phase == Phase::Prompting && current.cancelled
    }) else {
        return;
    };
    current.cancel_timer = None;
    if let Some(call) = current.prompt_call.take() {
        call.abort();
    }
    current.phase = Phase::Stopping;
    let has_agent = state(actor, thread).is_some_and(|state| state.agent.is_some());
    if has_agent {
        tracing::warn!(%thread, "the agent did not answer its cancel; stopping it");
        agents::stop(actor, thread);
    } else {
        end::end_turn(actor, thread, End::cancelled(None));
    }
}

/// Shutdown's part: running turns are cancelled (their agents stop when they end), turns that
/// have not started stay queued for the next start, and every other agent stops now.
pub(super) fn shutdown(actor: &mut Actor) {
    let threads: Vec<ThreadId> = actor
        .threads
        .iter()
        .filter(|(_, entry)| entry.turn.agent.is_some() || entry.turn.current.is_some())
        .map(|(thread, _)| *thread)
        .collect();
    for thread in threads {
        let found = current(actor, thread).map(|current| (current.phase, current.cancelled));
        match found {
            Some((Phase::Prompting, false)) => cancel_running(actor, thread, None),
            Some((Phase::Checkpointing, false)) => {
                if let Some(current) = current(actor, thread) {
                    current.cancelled = true;
                }
            }
            Some((Phase::Accepting | Phase::Launching | Phase::Queued, _)) => {
                if let Some(state) = state(actor, thread) {
                    state.current = None;
                }
                if let Ok(matter) = matter_of(actor, thread) {
                    end::release_writer(actor, matter, thread);
                }
                agents::stop(actor, thread);
            }
            Some(
                (Phase::Prompting | Phase::Checkpointing, true)
                | (Phase::Stopping | Phase::Ending, _),
            ) => {}
            None => agents::stop(actor, thread),
        }
    }
}
