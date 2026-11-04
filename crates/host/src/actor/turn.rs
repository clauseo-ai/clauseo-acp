//! The turn path: prompts and their turns, the agents that run them, the writer queue of each
//! matter, cancel, the commands that answer an agent, undo, start-up reconciliation and the turn
//! path's part of shutdown.
//!
//! A turn moves through the steps of the brief's "The turn": the `UserMessage` commit, the
//! agent's launch (`agents.rs`), the writer queue and the before-turn checkpoint, the prompt, and
//! the end (`steps.rs`); a thread's queue of prompts waiting behind its turn, send now and steer
//! are in `queue.rs`. Agent updates are mapped in `updates.rs`, text goes through the
//! coalescer in `text.rs`, and every commit of a thread stream starts by flushing that thread's
//! held text (`out.rs`), so text and other events keep their order.

mod agents;
mod answers;
mod asks;
mod cancel;
mod close;
mod end;
mod failure;
mod forced_mode;
mod inputs;
mod known;
mod launch_checks;
mod out;
mod profile_loads;
mod prompt;
mod queue;
mod reconcile;
mod restore;
mod route;
mod session;
mod state;
mod steps;
mod text;
mod undo;
mod undo_preview;
mod updates;
mod writer;

use std::collections::VecDeque;

use clauseo_protocol::{
    AgentChoiceId, ContentBlock, DetachReason, ElicitationResponse, ErrorCode, LimitStopReason,
    MatterId, MatterPath, StreamId, ThreadId, Timestamp, TurnId,
};
use futures::future::AbortHandle;

use super::{Actor, HostPhase};
use crate::agent::AgentInput;
use crate::coalesce::ThreadText;
use crate::error::Error;
use crate::matter_worker::MatterReply;
use crate::pipeline::Failure;

pub(crate) use asks::asks_pending;
pub(crate) use close::{closing, log_limit_stop};
pub(crate) use end::limits_read;
pub(crate) use failure::blocked as stream_blocked;
pub(crate) use inputs::{AgentReply, LogFor, TurnInput};
pub(crate) use launch_checks::RunSweep;
pub(crate) use out::flush_text;
pub(crate) use queue::queue_held;
pub(crate) use restore::{
    RestoreAsk, committed as restore_committed, on_restored, restore_version,
};
pub(crate) use route::{TurnCommand, command};
pub(crate) use undo_preview::preview_undo;
pub(crate) use writer::{
    hold_for_relocate, matter_busy, release_relocate, stop_agents, writer_busy,
};

/// How long a cancelled turn waits for the agent's answer before the host stops the agent.
const CANCEL_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// A thread's turn state: its current turn, its agent, its text buffers and its undo note. One
/// per thread, made with `Default` when the thread is loaded from the store or created.
#[derive(Debug, Default)]
pub(crate) struct TurnState {
    /// The thread's agent, from its launch until its tasks post `Ended`.
    agent: Option<Box<agents::Agent>>,
    /// A launch whose folder checks run off the actor, before its agent exists.
    preparing: Option<Box<launch_checks::Preparing>>,
    /// The folders of the launched agent's session, from its launch until it ends.
    run: Option<launch_checks::RunFolders>,
    /// The thread's current turn: accepted and not ended.
    current: Option<Box<Current>>,
    /// The browser opens of the thread's turns that have not finished, for `PageVisited`. Kept
    /// past a turn's end, so a completion that comes late or in a replay finds its open and is
    /// still not recorded (`updates.rs` drops it by its origin).
    browser_opens: crate::sources::browser::Opens,
    /// Text not durable yet.
    text: ThreadText,
    /// The files put back since the last `TurnStarted`; `None` until first needed, then
    /// read from the loaded state.
    note: Option<Vec<MatterPath>>,
    /// The files restores of the thread's matter wrote since the last `TurnStarted`, held in
    /// memory only (`restore.rs`); the next prompt's note adds them.
    restored: Vec<MatterPath>,
    /// Start-up reconciliation of this thread is in progress.
    reconciling: bool,
    /// The last turn, until the commit holding its `TurnEnded` returns `Ok`. A new prompt is
    /// refused while it is here, so the open turn reconciliation looks for stays the last one.
    closing: Option<Closing>,
    /// An undo R8 has done, until the commit logging it returns `Ok` or its retry returns;
    /// the thread holds its matter's writer meanwhile.
    undo: Option<Box<undo::UndoLog>>,
    /// The prompts waiting behind the current turn, whether they are held, steers in flight.
    queue: queue::ThreadQueue,
}

/// A turn whose `TurnEnded` is sent but not durable yet.
#[derive(Debug)]
struct Closing {
    /// The close that commit holds; the retry pushes it again.
    close: close::TurnClose,
    /// That commit, or one ahead of it, failed with this code; the retry logs it.
    failed: Option<ErrorCode>,
    /// The one retry was tried.
    retried: bool,
}

/// A matter's turn state: who holds its writer and who waits for it. One per matter.
#[derive(Debug, Default)]
pub(crate) struct MatterTurns {
    /// The turn, undo, restore or relocate holding the writer.
    holder: Option<Holder>,
    /// Threads whose turns wait for the writer, first come first served.
    queue: VecDeque<ThreadId>,
    /// Start-up repairs held until every thread of the matter has been looked at.
    repairs: reconcile::Repairs,
    /// A restore R8 has done, until the commit logging it returns `Ok` or its retry returns.
    restore: Option<Box<restore::RestoreLog>>,
}

/// What holds a matter's writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Holder {
    /// A thread's turn or undo.
    Thread(ThreadId),
    /// A `RestoreVersion`, which belongs to no thread.
    Restore,
    /// A `RelocateMatter`, which belongs to no thread: from its checks until its commit returns
    /// or it is refused.
    Relocate,
}

/// The current turn of a thread.
#[derive(Debug)]
struct Current {
    turn: TurnId,
    /// The lawyer's message that opened it.
    message: clauseo_protocol::MessageId,
    /// The prompt's content, until step 5 sends it.
    prompt: Vec<ContentBlock>,
    phase: TurnPhase,
    /// The lawyer cancelled it (or shutdown did); it ends `Cancelled`.
    cancelled: bool,
    /// The lawyer's last permission answer in the turn was a reject kind and no Stop came
    /// after it: the agent's own `Cancelled` then ends the turn `NotAllowed` (Codex's only
    /// reject option ends the turn). Another answer or a Stop clears it.
    refused: bool,
    /// The first limit signal of the turn.
    limit: Option<LimitMark>,
    /// The places the turn's tool calls named, for the after-turn checkpoint.
    tool_paths: crate::sent::TurnPaths,
    /// The before-turn checkpoint, once step 4 has it.
    checkpoint_before: Option<clauseo_protocol::CheckpointId>,
    /// Drops the prompt's call (the cancel fallback).
    prompt_call: Option<AbortHandle>,
    /// The cancel fallback timer.
    cancel_timer: Option<AbortHandle>,
    /// The agent's descendants listed as the prompt went out, filled in by the prompt's task:
    /// a cancelled turn ends what is new since (`agent/leftovers.rs`).
    before_prompt: Option<BeforePrompt>,
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
    /// A Codex prompt failed with a JSON-RPC error: the turn's end waits for a limits read.
    limit_read: Option<end::LimitRead>,
    /// When it became the current turn, on the host's monotonic clock: the usage counts'
    /// `answer_ended.took` runs from here to `end::finish`.
    started: std::time::Duration,
    /// When its first answer text came, on the same clock: `answer_ended.first_words`.
    first_words: Option<std::time::Duration>,
}

/// Where the prompt's task leaves the listing of the agent's descendants; `None` until it is
/// taken, and when it failed.
type BeforePrompt = std::rc::Rc<std::cell::RefCell<Option<clauseo_process::Snapshot>>>;

/// Where a current turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TurnPhase {
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
    FallbackStop,
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
    /// The commit logging `turn`'s stop at the limit after its read: forget the pending stop.
    LimitStop(TurnId),
    /// The commit logging an undo: update the note, let the writer go.
    Undone,
    /// A queue command's commit (`queue.rs`).
    Queue(queue::After),
    /// A permission answer: tell the agent.
    Permission {
        agent: u64,
        key: clauseo_acp::PendingKey,
        option: AgentChoiceId,
    },
    /// A permission request the host cancelled for the lawyer: tell the agent.
    CancelPermission {
        agent: u64,
        key: clauseo_acp::PendingKey,
    },
    /// An elicitation answer: tell the agent.
    Elicitation {
        agent: u64,
        key: clauseo_acp::PendingKey,
        response: ElicitationResponse,
    },
}

/// A turn timer: the display and durable batches, the rate window, the cancel, a limits read.
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
    LimitRead(TurnId),
    /// A steer's answer did not come in time (`queue/steer.rs`).
    Steer(clauseo_protocol::MessageId),
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
    Permission(Box<asks::HeldOpenCheck>),
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
        TurnInput::Prepared { thread, id, result } => {
            launch_checks::on_prepared(actor, thread, id, result);
        }
        TurnInput::Log {
            thread,
            why,
            result,
        } => match why {
            LogFor::Replay => session::on_replay_log(actor, thread, result),
            LogFor::Reconcile => reconcile::on_log(actor, thread, result),
            LogFor::Undo(ask) => undo::on_log(actor, thread, *ask, result),
            LogFor::PreviewUndo { turn, reply } => {
                undo_preview::on_log(actor, thread, turn, reply, result);
            }
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
        TimerKind::LimitRead(turn) => end::on_limit_read_timer(actor, thread, turn),
        TimerKind::Steer(message) => queue::on_steer_timer(actor, thread, message),
    }
}

/// A turn job of `matter`'s worker answered.
pub(crate) fn matter_reply(actor: &mut Actor, _matter: MatterId, job: TurnJob, reply: MatterReply) {
    let TurnJob { thread, kind } = job;
    match (kind, reply) {
        (JobKind::Before(turn), MatterReply::BeforeTurn(result)) => {
            known::when_loaded(actor, thread, result, move |actor, result| {
                steps::on_before_turn(actor, thread, turn, result);
            });
        }
        (JobKind::After(turn), MatterReply::AfterTurn(result)) => {
            known::when_loaded(actor, thread, result, move |actor, result| {
                end::on_after_turn(actor, thread, turn, result);
            });
        }
        (JobKind::Reconcile(repair), MatterReply::AfterTurn(result)) => {
            known::when_loaded(actor, thread, result, move |actor, result| {
                reconcile::on_after_turn(actor, thread, *repair, result);
            });
        }
        (JobKind::Undo(ask), MatterReply::Undone(result)) => {
            undo::on_undone(actor, thread, *ask, result.map(|undone| *undone));
        }
        (JobKind::Permission(ask), MatterReply::HeldOpen(held)) => {
            asks::on_held_open(actor, thread, *ask, held);
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
        (Then::Launch(turn), Ok(())) => {
            super::counting::question_sent(actor, thread, clauseo_counts::schema::Delivery::Now);
            steps::on_accepted(actor, thread, turn);
        }
        (Then::Launch(turn), Err(_)) => steps::on_refused(actor, thread, turn),
        (Then::Started(turn), Ok(())) => steps::on_started(actor, thread, turn),
        (Then::Closing(turn), Ok(())) => failure::on_closed(actor, thread, turn),
        (Then::Closing(turn), Err(failed)) => {
            failure::on_closing_failed(actor, thread, turn, failed);
        }
        (Then::LimitStop(turn), outcome) => {
            super::limits::stop_committed(actor, thread, turn, outcome.is_ok());
        }
        (Then::Queue(after), outcome) => queue::committed(actor, thread, after, outcome.is_ok()),
        (Then::Undone, Ok(())) => undo::on_logged(actor, thread),
        (Then::Undone, Err(_)) => undo::on_failed(actor, thread),
        (Then::Permission { agent, key, option }, Ok(())) => {
            answers::tell_permission(actor, thread, agent, key, option);
        }
        (Then::CancelPermission { agent, key }, Ok(())) => {
            answers::tell_cancel_permission(actor, thread, agent, key);
        }
        (
            Then::Elicitation {
                agent,
                key,
                response,
            },
            Ok(()),
        ) => answers::tell_elicitation(actor, thread, agent, key, response),
        (
            Then::Permission { .. } | Then::CancelPermission { .. } | Then::Elicitation { .. },
            Err(_),
        )
        | (Then::Nothing, Ok(())) => {}
        (Then::Nothing | Then::Started(_), Err(failed)) => {
            failure::on_store_failure(actor, thread, failed);
        }
    }
}

/// `stream`'s in-flight commits have all returned after a failure: its pending head is the
/// durable head again and its sent view has rolled back. "Store failures" try once more here.
pub(crate) fn unblocked(actor: &mut Actor, stream: StreamId) {
    match stream {
        StreamId::Thread(thread) => failure::on_unblocked(actor, thread),
        StreamId::Matter(matter) => restore::retry(actor, matter),
    }
}

/// Start-up reconciliation of `thread`, called once its stream is loaded and before the host
/// serves. Marks the work in progress so that [`busy`] holds start-up until it is done.
pub(crate) fn reconcile(actor: &mut Actor, thread: ThreadId) {
    reconcile::reconcile(actor, thread);
}

/// Start-up will reconcile `thread`: its matter's repairs wait for it (see
/// [`reconcile::Repairs`]). Called for every thread before any is loaded.
pub(crate) fn expect_reconcile(actor: &mut Actor, thread: ThreadId) {
    reconcile::expect(actor, thread);
}

/// `thread` could not be loaded at start-up, so it will not be reconciled; its matter's
/// repairs no longer know which open turn started last.
pub(crate) fn reconcile_skipped(actor: &mut Actor, thread: ThreadId) {
    reconcile::skipped(actor, thread);
}

/// Shutdown, after the host stopped taking commands: turns stop waiting for limits reads, every
/// running turn is cancelled (the Cancel path), every agent stops, every pending stop at the
/// limit is logged. The actor closes the store once [`busy`] is false and commits returned.
pub(crate) fn shutdown(actor: &mut Actor) {
    end::stop_waiting_for_reads(actor);
    cancel::shutdown(actor);
    super::limits::decide_stops_now(actor);
}

/// Whether start-up (reconciliation) or shutdown (cancels, agent stops, unlogged stops at the
/// limit) must wait for the turn path. Scanned only while starting or stopping.
pub(crate) fn busy(actor: &Actor) -> bool {
    if matches!(actor.phase, HostPhase::Serving) {
        return false;
    }
    let stops = host_stopping(actor) && super::limits::stops_busy(actor);
    stops
        || actor.threads.values().any(|entry| {
            let state = &entry.turn;
            state.reconciling
                || state.agent.is_some()
                || state.preparing.is_some()
                || state.current.as_ref().is_some_and(|current| {
                    matches!(
                        current.phase,
                        TurnPhase::Checkpointing
                            | TurnPhase::Starting
                            | TurnPhase::Prompting
                            | TurnPhase::FallbackStop
                            | TurnPhase::Ending
                    )
                })
        })
}

/// Whether `thread` has a current turn, a last turn whose end is not durable yet, or queued
/// turns (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.current.is_some() || state.closing.is_some() || !state.queue.is_empty()
    })
}

/// Whether `thread`'s stream stays loaded: an agent, open turn, repair, retriable end, limit stop.
pub(crate) fn keeps_loaded(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.agent.is_some()
            || state.preparing.is_some()
            || state.current.is_some()
            || state.reconciling
            || state
                .closing
                .as_ref()
                .is_some_and(|closing| !closing.retried)
    }) || super::limits::holds_stop(actor, thread)
}

/// Whether `matter`'s stream, once loaded to check a reused checkpoint, stays loaded: while a
/// turn, an undo or a restore holds its writer, so the turn's next checkpoint reads it without
/// a reload.
pub(crate) fn keeps_matter_loaded(actor: &Actor, matter: MatterId) -> bool {
    actor
        .matters
        .get(&matter)
        .is_some_and(|entry| entry.turns.holder.is_some())
}

/// Whether the host is shutting down.
fn host_stopping(actor: &Actor) -> bool {
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
