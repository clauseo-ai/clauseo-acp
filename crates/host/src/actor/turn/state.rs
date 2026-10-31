//! The constructors of a turn's in-memory state: a new current turn, and the ways a turn ends.

use std::collections::BTreeSet;

use clauseo_protocol::{ContentBlock, DetachReason, ErrorCode, ExpiryReason, StopReason, TurnId};

use super::{Current, End, Phase};

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
    pub(super) fn new(turn: TurnId, prompt: Vec<ContentBlock>) -> Self {
        Self {
            turn,
            prompt,
            phase: Phase::Accepting,
            cancelled: false,
            limit: None,
            tool_paths: BTreeSet::new(),
            checkpoint_before: None,
            prompt_call: None,
            cancel_timer: None,
            end: None,
            failure: None,
            abort: false,
            store_failed: None,
            detach: None,
        }
    }

    /// Whether agent updates of the turn are logged: its prompt is in flight, not cancelled.
    pub(super) fn live(&self) -> bool {
        self.phase == Phase::Prompting && !self.cancelled
    }
}
