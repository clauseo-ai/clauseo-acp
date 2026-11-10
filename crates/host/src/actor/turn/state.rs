//! The constructors of a turn's in-memory state: a new current turn, and the ways a turn ends;
//! whether start-up or shutdown waits for the turn path; whether an update is the live turn's;
//! and what a thread's turn state holds.

use clauseo_protocol::{
    ContentBlock, DetachReason, ErrorCode, ExpiryReason, MatterId, MessageId, StopReason, ThreadId,
    TurnId,
};

use super::super::{Actor, HostPhase};
use super::{Current, End, TurnPhase, host_stopping, key_retry};

/// The detail of a `TurnFailed` logged because a commit of the turn failed.
pub(super) const STORE_DETAIL: &str = "a commit of the turn failed";

impl End {
    /// A turn the lawyer (or shutdown) cancelled.
    pub(super) fn cancelled(detach: Option<DetachReason>) -> Self {
        Self {
            stop: StopReason::Cancelled,
            failure: None,
            expiry: ExpiryReason::TurnEnded,
            detach,
        }
    }

    /// A turn that failed with `code`.
    pub(super) fn failed(
        code: ErrorCode,
        detail: Option<String>,
        detach: Option<DetachReason>,
    ) -> Self {
        let expiry = if code == ErrorCode::AgentExited {
            ExpiryReason::AgentExited
        } else {
            ExpiryReason::TurnEnded
        };
        Self {
            stop: StopReason::Failed,
            failure: Some((code, detail)),
            expiry,
            detach,
        }
    }

    /// A commit of the turn failed with `code`: the turn ends `Failed` with it, unless it
    /// already ends with a failure of its own (the first failure is the one logged).
    pub(super) fn store_failed(&mut self, code: Option<ErrorCode>) {
        if let Some(code) = code
            && self.failure.is_none()
        {
            self.stop = StopReason::Failed;
            self.failure = Some((code, Some(STORE_DETAIL.to_owned())));
        }
    }
}

/// Where the prompt's task leaves the listing of the agent's descendants; `None` until it is
/// taken, and when it failed.
pub(super) type BeforePrompt = std::rc::Rc<std::cell::RefCell<Option<clauseo_process::Snapshot>>>;

impl Current {
    pub(super) fn new(
        turn: TurnId,
        message: MessageId,
        prompt: Vec<ContentBlock>,
        started: std::time::Duration,
    ) -> Self {
        Self {
            turn,
            message,
            prompt,
            phase: TurnPhase::Accepting,
            cancelled: false,
            stopped: key_retry::Stopped::default(),
            refused: false,
            limit: None,
            tool_paths: crate::sent::TurnPaths::default(),
            checkpoint_before: None,
            prompt_call: None,
            case_law_hint: crate::connectors::hint::TurnLooks::Nothing,
            cancel_timer: None,
            before_prompt: None,
            end: None,
            failure: None,
            offline: None,
            abort: false,
            store_failed: None,
            detach: None,
            limit_read: None,
            started,
            first_words: None,
            first_update: None,
            withheld: super::withheld::Withheld::default(),
        }
    }

    /// Whether agent updates of the turn are logged: its prompt is in flight, not cancelled.
    pub(super) fn live(&self) -> bool {
        self.phase == TurnPhase::Prompting && !self.cancelled
    }
}

/// Whether start-up (reconciliation) or shutdown (cancels, agent stops, unlogged stops at the
/// limit) must wait for the turn path. Scanned only while starting or stopping.
pub(crate) fn busy(actor: &Actor) -> bool {
    if matches!(actor.phase, HostPhase::Serving) {
        return false;
    }
    let stops = host_stopping(actor) && super::super::limits::stops_busy(actor);
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

/// Whether an update of `turn` belongs to `thread`'s live turn (its prompt in flight, not
/// cancelled). The first one has its time kept and a lifecycle line marks it (D-022).
pub(super) fn live(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let now = actor.now().mono;
    let Some(current) =
        super::current(actor, thread).filter(|current| current.turn == turn && current.live())
    else {
        return false;
    };
    if current.first_update.is_none() {
        current.first_update = Some(now);
        tracing::info!(target: crate::log_file::LIFECYCLE, %thread, %turn, "first update");
    }
    true
}

/// Whether `thread` has a current turn, a last turn whose end is not durable yet, or queued
/// turns (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.current.is_some() || state.closing.is_some() || !state.queue.is_empty()
    })
}

/// The turn running on `thread` now, if any (`blocked.rs` attributes a blocked site to it).
pub(crate) fn running_turn(actor: &Actor, thread: ThreadId) -> Option<TurnId> {
    let current = actor.threads.get(&thread)?.turn.current.as_deref()?;
    Some(current.turn)
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
    }) || super::super::limits::holds_stop(actor, thread)
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
