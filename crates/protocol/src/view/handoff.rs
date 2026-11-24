//! The handoff entry: the earlier conversation handed to the agent a thread switched to.

use serde::{Deserialize, Serialize};

use crate::{Count, Endpoint, HandoffId, HandoffStatus, MessageId, Seq, Timestamp};

/// A handover of the conversation's earlier items to the agent a thread switched to, from its
/// `ContextHandoff`; its `HandoffSettled` sets `status`. It goes with the lawyer's
/// `message`, and the view shows it just before that message. Entry id `handoff:<handoff>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct HandoffEntry {
    /// The `seq` of `ContextHandoff`.
    pub order: Seq,
    /// The handover.
    pub handoff: HandoffId,
    /// The lawyer's message it goes with.
    pub message: MessageId,
    /// The agent it comes from.
    pub from: Endpoint,
    /// The agent it goes to.
    pub to: Endpoint,
    /// The earlier answers the conversation held.
    pub answers_covered: Count,
    /// The earlier items the handover holds.
    pub items_included: Count,
    /// The earlier items it leaves out.
    pub items_left_out: Count,
    /// Pending until its `HandoffSettled`, then delivered or failed.
    pub status: HandoffEntryStatus,
    /// When it was logged (display only; the `ContextHandoff` envelope's `ts`).
    pub ts: Timestamp,
}

/// Where a handover stands, as the view shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum HandoffEntryStatus {
    /// Its prompt has not gone out yet.
    Pending,
    /// Its prompt went out to the new agent.
    Delivered,
    /// The turn failed before its prompt went out.
    Failed,
}

impl From<HandoffStatus> for HandoffEntryStatus {
    /// A status from a newer Clauseo reads as failed: the handover did not show as delivered.
    fn from(status: HandoffStatus) -> Self {
        match status {
            HandoffStatus::Delivered => Self::Delivered,
            HandoffStatus::Failed | HandoffStatus::Unknown => Self::Failed,
        }
    }
}
