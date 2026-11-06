//! The constructors of a turn's in-memory state: a new current turn, and the ways a turn ends;
//! and whether start-up or shutdown waits for the turn path.

use clauseo_protocol::{
    ContentBlock, DetachReason, ErrorCode, ExpiryReason, MessageId, StopReason, TurnId,
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
            abort: false,
            store_failed: None,
            detach: None,
            limit_read: None,
            started,
            first_words: None,
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
