//! The turn path's seams. The actor calls each function below at the point named in its doc;
//! the turn path fills them in. Until then every turn command answers `internal` "not built"
//! and nothing else happens.
//!
//! What the actor gives the turn path (all on [`Actor`]): `now()`, `ids`, `post` (the internal
//! input channel: an agent's `post` wraps each `AgentInput` in [`Input::Agent`]), `spawner`,
//! `arm_timer` (a [`TurnTimer`] posted as [`Input::Timer`] after a delay on the host clock),
//! `matter_worker` (the matter's thread, opened on first use; jobs tagged [`TurnJob`] come back
//! to [`matter_reply`]), `pipeline.push` and `send_commit` (a commit tagged [`TurnTag`] comes
//! back to [`committed`]), `with_loaded` and `release` (load a stream, let it unload),
//! `streams` (the loaded state, view and sent view of a stream), `finish_command` and
//! `CommandCtx::receipt` for the commands it serves.
//!
//! [`Input::Agent`]: super::input::Input::Agent
//! [`Input::Timer`]: super::input::Input::Timer

use clauseo_protocol::{Envelope, HostCommand, MatterId, StreamId, ThreadId};

use super::Actor;
use super::commands::CommandCtx;
use crate::agent::AgentInput;
use crate::error::Error;
use crate::matter_worker::MatterReply;
use crate::pipeline::Failure;

/// A thread's turn state: its current turn, its agent, its text buffers and its timers. One per
/// thread, made with `Default` when the thread is loaded from the store or created.
#[derive(Debug, Default)]
pub(crate) struct TurnState {}

/// A matter's turn state: who holds its writer and who waits for it. One per matter.
#[derive(Debug, Default)]
pub(crate) struct MatterTurns {}

/// What a turn commit continues with once its result is back.
#[derive(Debug)]
pub(crate) enum TurnTag {}

/// A turn timer: the display and durable batches, the rate window, the cancel fallback.
#[derive(Debug)]
pub(crate) enum TurnTimer {}

/// What a matter job of a turn (a checkpoint, an undo) continues with.
#[derive(Debug)]
pub(crate) enum TurnJob {}

/// A turn command, after its shape, principal and receipt checks passed and its thread is
/// known: `SendPrompt`, `Cancel`, `AnswerPermission`, `AnswerElicitation`, `SetConfigOption`,
/// `SetMode`, `UndoStep`. It must end in `actor.finish_command(ctx.id, ..)`, at once for a
/// refusal (no receipt) or from the continuation of a commit that carries
/// `ctx.receipt(..)` and the tag `Tag::Command { after: AfterCommand::Turn(..), .. }`.
pub(crate) fn command(
    actor: &mut Actor,
    ctx: &CommandCtx,
    _thread: ThreadId,
    command: HostCommand,
) {
    drop(command);
    actor.finish_command(ctx.id, Err(Error::Internal("not built yet".to_owned())));
}

/// An agent's event or its end, in the order its tasks posted them, for `thread`.
pub(crate) fn agent_input(_actor: &mut Actor, _thread: ThreadId, input: AgentInput) {
    drop(input);
}

/// A timer armed with `Actor::arm_timer` fired.
pub(crate) fn timer(_actor: &mut Actor, timer: TurnTimer) {
    let never = timer;
    match never {}
}

/// A turn job of `matter`'s worker answered.
pub(crate) fn matter_reply(
    _actor: &mut Actor,
    _matter: MatterId,
    job: TurnJob,
    reply: MatterReply,
) {
    drop(reply);
    let never = job;
    match never {}
}

/// A commit tagged `tag` returned: `Ok` with its envelopes, already applied and published, or
/// the failure (its `error` is the stream's first failure). For a command commit the actor
/// answers the command right after this call.
pub(crate) fn committed(_actor: &mut Actor, tag: TurnTag, _outcome: Result<&[Envelope], &Failure>) {
    let never = tag;
    match never {}
}

/// `stream`'s in-flight commits have all returned after a failure: its pending head is the
/// durable head again and its sent view has rolled back. "Store failures" try once more here.
pub(crate) fn unblocked(_actor: &mut Actor, _stream: StreamId) {}

/// Start-up reconciliation of `thread`, called once its stream is loaded and before the host
/// serves. Mark the work in progress so that [`busy`] holds start-up until it is done.
pub(crate) fn reconcile(_actor: &mut Actor, _thread: ThreadId) {}

/// Shutdown, after the host stopped taking commands: cancel every running turn through the
/// Cancel path and stop every agent. The actor closes the store once [`busy`] is false and
/// every commit has returned.
pub(crate) fn shutdown(_actor: &mut Actor) {}

/// Whether the turn path has work that start-up (reconciliation) or shutdown (cancels, agent
/// stops) must wait for.
pub(crate) fn busy(_actor: &Actor) -> bool {
    false
}

/// Whether `thread` has a current turn (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(_actor: &Actor, _thread: ThreadId) -> bool {
    false
}

/// Whether `matter`'s writer is held by a turn or an undo (`matter.turn_running`).
#[expect(dead_code, reason = "the turn path's undo check uses it")]
pub(crate) fn writer_busy(_actor: &Actor, _matter: MatterId) -> bool {
    false
}

/// Whether `thread`'s stream must stay loaded: a live agent or an open turn.
pub(crate) fn keeps_loaded(_actor: &Actor, _thread: ThreadId) -> bool {
    false
}
