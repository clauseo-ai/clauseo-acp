//! A turn's in-memory state: where a current turn is, how it ends, the constructors of a new
//! current turn and of the ways a turn ends; and whether start-up or shutdown waits for the turn
//! path.

use clauseo_protocol::{
    ContentBlock, DetachReason, ErrorCode, ExpiryReason, LimitStopReason, MessageId, StopReason,
    Timestamp, TurnId,
};

use super::super::{Actor, HostPhase};
use super::{Current, host_stopping, key_retry};

/// Where a current turn is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TurnPhase {
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

/// Whether a turn's agent's process stayed in the Safe workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Workspace {
    /// It did, or it ran without the sandbox runner.
    Inside,
    /// It ended with the sandbox runner's Escapee exit (`agents/ended.rs`).
    Left,
}

/// A limit signal that stops the turn at the limit.
#[derive(Debug, Clone)]
pub(super) struct LimitMark {
    pub(super) window: Option<String>,
    pub(super) resets_at: Option<Timestamp>,
    pub(super) reason: LimitStopReason,
}

/// How a turn ends.
#[derive(Debug, Clone)]
pub(super) struct End {
    pub(super) stop: StopReason,
    pub(super) failure: Option<(ErrorCode, Option<String>)>,
    pub(super) expiry: ExpiryReason,
    /// Logged after `TurnEnded`: the agent exited during the turn.
    pub(super) detach: Option<DetachReason>,
}

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

    /// A turn whose agent's process left the Safe workspace (the sandbox runner's Escapee exit,
    /// `agents/ended.rs`): `LeftSafeWorkspace`, with no `TurnFailed`; its asks expire as for
    /// an agent that exited.
    pub(super) fn left_safe_workspace(detach: Option<DetachReason>) -> Self {
        Self {
            stop: StopReason::LeftSafeWorkspace,
            failure: None,
            expiry: ExpiryReason::AgentExited,
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
            workspace: Workspace::Inside,
            limit_read: None,
            exit_wait: super::exit_wait::ExitWait::default(),
            started,
            first_words: None,
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
