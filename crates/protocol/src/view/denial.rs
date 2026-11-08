//! The view of denials (R18): a refusal's entry, its card's state, and a paused answer.

use serde::{Deserialize, Serialize};

use crate::open_enum::enum_with_names;
use crate::{
    DenialId, DenialKind, DenialSource, EscalationAnswer, EventId, Seq, Timestamp, TurnId,
};

enum_with_names! {
    /// Where a denial's entry stands. Not logged: a new state is additive under 0002's freeze
    /// rules, since the desktop and the cli ship with the host.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    #[cfg_attr(feature = "specta", derive(specta::Type))]
    pub enum DenialState {
        /// A card waits for the lawyer: "Allow access…?", which asks the host for the dialog.
        Pending,
        /// Logged for "What was refused" with no card: the host never widens it.
        NotOffered,
        /// The lawyer allowed it in the dialog (`answer` says whether for the matter).
        Allowed,
        /// The lawyer refused it in the dialog, closed it, or pressed Esc.
        Refused,
        /// The lawyer dismissed the card; the webview hides it.
        Dismissed,
        /// The agent stopped before the card was answered; nothing is left to widen.
        Expired,
    }
}

/// One refused path or site. Entry id `denial:<denial>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DenialEntry {
    /// The `seq` of its first `DenialRecorded`.
    pub order: Seq,
    /// The denial.
    pub denial: DenialId,
    /// The turn it came in.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub turn: Option<TurnId>,
    /// What was refused.
    pub kind: DenialKind,
    /// Who reported it.
    pub source: DenialSource,
    /// Where it stands.
    pub state: DenialState,
    /// The dialog's answer, once there is one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub answer: Option<EscalationAnswer>,
    /// How many refusals of it the turn has seen (the latest `DenialRecorded`'s `count`).
    pub count: u32,
    /// When it was first logged (display only; that envelope's `ts`).
    pub ts: Timestamp,
}

/// The host paused an answer after too many refusals. Entry id `denials_paused:<event>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DenialsPausedEntry {
    /// The `seq` of `DenialsPaused`.
    pub order: Seq,
    /// That event's id.
    pub event: EventId,
    /// The turn.
    pub turn: TurnId,
    /// The refusals counted toward the pause.
    pub refused: u16,
    /// When it was logged (display only; the envelope's `ts`).
    pub ts: Timestamp,
}
