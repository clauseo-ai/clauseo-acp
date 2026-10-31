//! The turn path: prompts and their turns, the agents that run them, the writer queue of each
//! matter, cancel, the commands that answer an agent, undo, start-up reconciliation and the turn
//! path's part of shutdown.
//!
//! A turn moves through the steps of the brief's "The turn": the `UserMessage` commit, the
//! agent's launch (`agents.rs`), the writer queue and the before-turn checkpoint, the prompt, and
//! the end (`steps.rs`). Agent updates are mapped in `updates.rs`, text goes through the
//! coalescer in `text.rs`, and every commit of a thread stream starts by flushing that thread's
//! held text (`out.rs`), so text and other events keep their order.

mod agents;
mod answers;
mod asks;
mod cancel;
mod close;
mod end;
mod failure;
mod inputs;
mod out;
mod prompt;
mod reconcile;
mod session;
mod state;
mod steps;
mod text;
mod undo;
mod updates;

use std::collections::{BTreeSet, VecDeque};

use clauseo_protocol::{
    AgentChoiceId, ContentBlock, DetachReason, ElicitationResponse, ErrorCode, ExpiryReason,
    HostCommand, LimitStopReason, MatterId, MatterPath, StreamId, ThreadId, Timestamp, TurnId,
};
use futures::future::AbortHandle;

use super::commands::CommandCtx;
use super::{Actor, Phase as HostPhase};
use crate::agent::AgentInput;
use crate::coalesce::ThreadText;
use crate::error::Error;
use crate::matter_worker::MatterReply;
use crate::pipeline::Failure;

pub(crate) use inputs::{AgentReply, LogFor, TurnInput};
pub(crate) use out::flush_text;

/// How long a cancelled turn waits for the agent's answer before the host stops the agent.
const CANCEL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A thread's turn state: its current turn, its agent, its text buffers and its undo note. One
/// per thread, made with `Default` when the thread is loaded from the store or created.
#[derive(Debug, Default)]
pub(crate) struct TurnState {
    /// The thread's agent, from its launch until its tasks post `Ended`.
    agent: Option<Box<agents::Agent>>,
    /// The thread's current turn: accepted and not ended.
    current: Option<Box<Current>>,
    /// Text not durable yet.
    text: ThreadText,
    /// The files put back since the last `TurnStarted`; `None` until first needed, then
    /// read from the loaded state.
    note: Option<Vec<MatterPath>>,
    /// Start-up reconciliation of this thread is in progress.
    reconciling: bool,
    /// The last turn, until the commit holding its `TurnEnded` returns `Ok`. A new prompt is
    /// refused while it is here, so the open turn reconciliation looks for stays the last one.
    closing: Option<Closing>,
}

/// A turn whose `TurnEnded` is sent but not durable yet.
#[derive(Debug)]
struct Closing {
    turn: TurnId,
    /// That commit, or one ahead of it, failed with this code; the retry logs it.
    failed: Option<ErrorCode>,
    /// The one retry was tried.
    retried: bool,
    /// The reason the retry's expiries give.
    expiry: ExpiryReason,
    /// The detach the end logs last, if the agent exited.
    detach: Option<DetachReason>,
}

/// A matter's turn state: who holds its writer and who waits for it. One per matter.
#[derive(Debug, Default)]
pub(crate) struct MatterTurns {
    /// The thread whose turn or undo holds the writer.
    holder: Option<ThreadId>,
    /// Threads whose turns wait for the writer, first come first served.
    queue: VecDeque<ThreadId>,
}

/// The current turn of a thread.
#[derive(Debug)]
struct Current {
    turn: TurnId,
    /// The prompt's content, until step 5 sends it.
    prompt: Vec<ContentBlock>,
    phase: Phase,
    /// The lawyer cancelled it (or shutdown did); it ends `Cancelled`.
    cancelled: bool,
    /// The first limit signal of the turn.
    limit: Option<LimitMark>,
    /// The matter paths the turn's tool calls named, for the after-turn checkpoint.
    tool_paths: BTreeSet<MatterPath>,
    /// Drops the prompt's call (the cancel fallback).
    prompt_call: Option<AbortHandle>,
    /// The cancel fallback timer.
    cancel_timer: Option<AbortHandle>,
    /// How the turn ends, once known.
    end: Option<End>,
    /// The turn failed on the host's side; it ends `Failed` with this `TurnFailed`.
    failure: Option<(ErrorCode, Option<String>)>,
    /// The failure asks to stop the prompt once the commit being built is sent.
    abort: bool,
    /// An agent-driven commit of the turn failed with this code.
    store_failed: Option<ErrorCode>,
    /// The agent exited before the prompt went out; logged last, after `TurnEnded`.
    detach: Option<DetachReason>,
}

/// Where a current turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Its `UserMessage` commit is in flight.
    Accepting,
    /// Its agent is launching (step 2).
    Launching,
    /// It waits for its matter's writer (step 3).
    Queued,
    /// Its before-turn checkpoint runs (step 4).
    Checkpointing,
    /// Its `TurnStarted` commit is in flight; the prompt goes out once it is durable.
    Starting,
    /// Its prompt is in flight (step 5).
    Prompting,
    /// The cancel fallback fired: the agent is being stopped.
    Stopping,
    /// Steps 6 to 8.
    Ending,
}

/// A limit signal that stops the turn at the limit.
#[derive(Debug, Clone)]
struct LimitMark {
    window: Option<String>,
    resets_at: Option<Timestamp>,
    reason: LimitStopReason,
}

/// How a turn ends.
#[derive(Debug, Clone)]
struct End {
    stop: clauseo_protocol::StopReason,
    failure: Option<(ErrorCode, Option<String>)>,
    expiry: clauseo_protocol::ExpiryReason,
    /// Logged after `TurnEnded`: the agent exited during the turn.
    detach: Option<clauseo_protocol::DetachReason>,
}

/// What a turn commit continues with once its result is back.
#[derive(Debug)]
pub(crate) struct TurnTag {
    thread: ThreadId,
    then: Then,
}

#[derive(Debug)]
enum Then {
    /// Nothing: an agent update, a step of a turn.
    Nothing,
    /// The `UserMessage` of `turn`: launch its agent.
    Launch(TurnId),
    /// The `TurnStarted` of `turn`: send the prompt.
    Started(TurnId),
    /// The commit holding the `TurnEnded` of `turn`: forget its closing record.
    Closing(TurnId),
    /// A permission answer: tell the agent.
    Permission {
        agent: u64,
        key: clauseo_acp::PendingKey,
        option: AgentChoiceId,
    },
    /// An elicitation answer: tell the agent.
    Elicitation {
        agent: u64,
        key: clauseo_acp::PendingKey,
        response: ElicitationResponse,
    },
}

/// A turn timer: the display and durable batches, the rate window, the cancel fallback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TurnTimer {
    thread: ThreadId,
    kind: TimerKind,
}

#[derive(Debug, Clone, Copy)]
enum TimerKind {
    Display,
    Durable,
    Rate,
    Cancel(TurnId),
}

/// What a matter job of a turn (a checkpoint, an undo) continues with.
#[derive(Debug)]
pub(crate) struct TurnJob {
    thread: ThreadId,
    kind: JobKind,
}

#[derive(Debug)]
enum JobKind {
    Before(TurnId),
    After(TurnId),
    Reconcile(Box<reconcile::Repair>),
    Undo(Box<undo::UndoAsk>),
}

/// A turn command, after its shape, principal and receipt checks passed and its thread is
/// known: `SendPrompt`, `Cancel`, `AnswerPermission`, `AnswerElicitation`, `SetConfigOption`,
/// `SetMode`, `UndoStep`. It ends in `actor.finish_command(ctx.id, ..)`, at once for a
/// refusal (no receipt) or from the continuation of a commit that carries the receipt.
pub(crate) fn command(actor: &mut Actor, ctx: &CommandCtx, thread: ThreadId, command: HostCommand) {
    let ctx = *ctx;
    let done = match command {
        HostCommand::SendPrompt {
            content, delivery, ..
        } => steps::send_prompt(actor, ctx, thread, content, delivery.into()),
        HostCommand::Cancel { .. } => cancel::cancel_command(actor, ctx, thread),
        HostCommand::AnswerPermission {
            request, option_id, ..
        } => answers::answer_permission(actor, ctx, thread, request, option_id),
        HostCommand::AnswerElicitation {
            elicitation,
            response,
            ..
        } => answers::answer_elicitation(actor, ctx, thread, elicitation, response),
        HostCommand::SetConfigOption {
            option_id, value, ..
        } => {
            agents::ensure(
                actor,
                thread,
                agents::Waiting::Config(ctx, option_id, value),
            );
            Ok(())
        }
        HostCommand::SetMode { mode_id, .. } => {
            agents::ensure(actor, thread, agents::Waiting::Mode(ctx, mode_id));
            Ok(())
        }
        HostCommand::UndoStep { turn, files, .. } => {
            undo::undo_step(actor, ctx, thread, turn, files)
        }
        HostCommand::CreateMatter { .. }
        | HostCommand::CreateThread { .. }
        | HostCommand::EditQueuedPrompt { .. }
        | HostCommand::RemoveQueuedPrompt { .. }
        | HostCommand::SendQueuedNow { .. }
        | HostCommand::ResumeQueue { .. }
        | HostCommand::RestartAgent { .. }
        | HostCommand::RenameThread { .. }
        | HostCommand::ArchiveThread { .. }
        | HostCommand::RestoreVersion { .. } => {
            Err(Error::Internal("not a turn command".to_owned()))
        }
    };
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// An agent's event or its end, in the order its tasks posted them, for `thread`.
pub(crate) fn agent_input(actor: &mut Actor, thread: ThreadId, input: AgentInput) {
    match input {
        AgentInput::Event(event) => updates::on_event(actor, thread, event),
        AgentInput::Ended(ended) => agents::on_ended(actor, thread, ended),
    }
}

/// A result of the turn path's own tasks.
pub(crate) fn turn_input(actor: &mut Actor, input: TurnInput) {
    match input {
        TurnInput::Agent {
            thread,
            agent,
            reply,
        } => agents::on_reply(actor, thread, agent, reply),
        TurnInput::Log {
            thread,
            why,
            result,
        } => match why {
            LogFor::Replay => session::on_replay_log(actor, thread, result),
            LogFor::Reconcile => reconcile::on_log(actor, thread, result),
            LogFor::Undo(ask) => undo::on_log(actor, thread, *ask, result),
        },
    }
}

/// A timer armed with `Actor::arm_timer` fired.
pub(crate) fn timer(actor: &mut Actor, timer: TurnTimer) {
    let TurnTimer { thread, kind } = timer;
    match kind {
        TimerKind::Display => text::on_display_timer(actor, thread),
        TimerKind::Durable => text::on_durable_timer(actor, thread),
        TimerKind::Rate => updates::on_rate_timer(actor, thread),
        TimerKind::Cancel(turn) => cancel::on_cancel_timer(actor, thread, turn),
    }
}

/// A turn job of `matter`'s worker answered.
pub(crate) fn matter_reply(actor: &mut Actor, _matter: MatterId, job: TurnJob, reply: MatterReply) {
    let TurnJob { thread, kind } = job;
    match (kind, reply) {
        (JobKind::Before(turn), MatterReply::BeforeTurn(result)) => {
            steps::on_before_turn(actor, thread, turn, result);
        }
        (JobKind::After(turn), MatterReply::AfterTurn(result)) => {
            end::on_after_turn(actor, thread, turn, result);
        }
        (JobKind::Reconcile(repair), MatterReply::AfterTurn(result)) => {
            reconcile::on_after_turn(actor, thread, *repair, result);
        }
        (JobKind::Undo(ask), MatterReply::Undone(result)) => {
            undo::on_undone(actor, thread, *ask, result);
        }
        (kind, reply) => {
            tracing::error!(
                ?kind,
                ?reply,
                "a matter job answered with another job's reply"
            );
        }
    }
}

/// A commit tagged `tag` returned: `Ok` (its envelopes already applied and published), or
/// the failure (its `error` is the stream's first failure). For a command commit the actor
/// answers the command right after this call.
pub(crate) fn committed(actor: &mut Actor, tag: TurnTag, outcome: Result<(), &Failure>) {
    let TurnTag { thread, then } = tag;
    match (then, outcome) {
        (Then::Launch(turn), Ok(())) => steps::on_accepted(actor, thread, turn),
        (Then::Launch(turn), Err(_)) => steps::on_refused(actor, thread, turn),
        (Then::Started(turn), Ok(())) => steps::on_started(actor, thread, turn),
        (Then::Closing(turn), Ok(())) => failure::on_closed(actor, thread, turn),
        (Then::Closing(turn), Err(failed)) => {
            failure::on_closing_failed(actor, thread, turn, failed);
        }
        (Then::Permission { agent, key, option }, Ok(())) => {
            answers::tell_permission(actor, thread, agent, key, option);
        }
        (
            Then::Elicitation {
                agent,
                key,
                response,
            },
            Ok(()),
        ) => answers::tell_elicitation(actor, thread, agent, key, response),
        (Then::Permission { .. } | Then::Elicitation { .. }, Err(_)) | (Then::Nothing, Ok(())) => {}
        (Then::Nothing | Then::Started(_), Err(failed)) => {
            failure::on_store_failure(actor, thread, failed);
        }
    }
}

/// `stream`'s in-flight commits have all returned after a failure: its pending head is the
/// durable head again and its sent view has rolled back. "Store failures" try once more here.
pub(crate) fn unblocked(actor: &mut Actor, stream: StreamId) {
    if let StreamId::Thread(thread) = stream {
        failure::on_unblocked(actor, thread);
    }
}

/// Start-up reconciliation of `thread`, called once its stream is loaded and before the host
/// serves. Marks the work in progress so that [`busy`] holds start-up until it is done.
pub(crate) fn reconcile(actor: &mut Actor, thread: ThreadId) {
    reconcile::reconcile(actor, thread);
}

/// Shutdown, after the host stopped taking commands: cancel every running turn through the
/// Cancel path and stop every agent. The actor closes the store once [`busy`] is false and
/// every commit has returned.
pub(crate) fn shutdown(actor: &mut Actor) {
    cancel::shutdown(actor);
}

/// Whether the turn path has work that start-up (reconciliation) or shutdown (cancels, agent
/// stops) must wait for. Only scanned while starting or stopping; serving never waits on it.
pub(crate) fn busy(actor: &Actor) -> bool {
    if matches!(actor.phase, HostPhase::Serving) {
        return false;
    }
    actor.threads.values().any(|entry| {
        let state = &entry.turn;
        state.reconciling
            || state.agent.is_some()
            || state.current.as_ref().is_some_and(|current| {
                matches!(
                    current.phase,
                    Phase::Checkpointing
                        | Phase::Starting
                        | Phase::Prompting
                        | Phase::Stopping
                        | Phase::Ending
                )
            })
    })
}

/// Whether `thread` has a current turn, or a last turn whose end is not durable yet
/// (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.turn.current.is_some() || entry.turn.closing.is_some())
}

/// Whether `thread`'s stream must stay loaded: a live agent, an open turn, a repair, an end
/// that may still be retried.
pub(crate) fn keeps_loaded(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.agent.is_some()
            || state.current.is_some()
            || state.reconciling
            || state
                .closing
                .as_ref()
                .is_some_and(|closing| !closing.retried)
    })
}

/// Whether the host is shutting down.
fn stopping(actor: &Actor) -> bool {
    matches!(actor.phase, HostPhase::Stopping(_))
}

/// The thread's turn state.
fn state(actor: &mut Actor, thread: ThreadId) -> Option<&mut TurnState> {
    actor.threads.get_mut(&thread).map(|entry| &mut entry.turn)
}

/// The thread's current turn.
fn current(actor: &mut Actor, thread: ThreadId) -> Option<&mut Current> {
    state(actor, thread).and_then(|state| state.current.as_deref_mut())
}

/// The matter of `thread`.
fn matter_of(actor: &Actor, thread: ThreadId) -> Result<MatterId, Error> {
    Ok(actor.check_thread(thread)?.row.matter)
}

/// Runs `task` on the host's pool; its result is not posted back.
fn spawn(actor: &Actor, task: impl std::future::Future<Output = ()> + 'static) {
    use futures::task::LocalSpawnExt;
    if let Err(error) = actor.spawner.spawn_local(task) {
        tracing::error!(%error, "the host pool refused a task");
    }
}

/// An internal error for a state the turn path should never be in.
fn lost(what: &str) -> Error {
    Error::Internal(format!("the turn path lost {what}"))
}

#[cfg(test)]
mod tests;
