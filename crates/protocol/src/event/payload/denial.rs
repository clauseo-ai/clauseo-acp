//! Payloads of denials and widening (R18): in a thread's stream, a refusal, the answer to its
//! dialog, a dismissed card and a paused answer; in a matter's stream, the answers it
//! remembers, the reads another matter was granted, and protection switched off or on.
//!
//! None repeats its stream's id: the envelope's `stream` names the thread or the matter.

use serde::{Deserialize, Serialize};

use crate::{
    AllowTarget, AllowanceId, DenialId, DenialKind, DenialSource, EscalationAnswer,
    EscalationRequestId, GrantMode, MatterId, TurnId,
};

/// The jail or the egress proxy refused a path or a site (thread stream). Logged for every
/// refusal, carded or not, for "What was refused"; `offered` says whether its card may widen
/// it. The same `denial` logged again in its turn carries a higher `count`: one card, counted.
/// JSON: `{"denial_recorded": {"denial": "…", "kind": {"site": {"host": "example.com"}},
/// "source": "proxy", "offered": true, "count": 1}}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DenialRecorded {
    /// The turn it came in; left out when no turn ran (an idle agent's own work).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub turn: Option<TurnId>,
    /// The denial.
    pub denial: DenialId,
    /// What was refused.
    pub kind: DenialKind,
    /// Who reported it.
    pub source: DenialSource,
    /// Whether a card may offer to widen it. `false` for what the host never widens: a secret
    /// place, the firm's policy, a refused folder, a remembered refusal.
    pub offered: bool,
    /// How many refusals of it the turn has seen, from 1.
    pub count: u32,
}

/// The lawyer answered a native dialog (thread stream). `denial` names the card it came from;
/// a matter-level escalation (switching protection off, another matter's files) is recorded by
/// what it changed, in the matter's stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EscalationAnswered {
    /// The escalation.
    pub request: EscalationRequestId,
    /// The denial whose card asked, when one did.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub denial: Option<DenialId>,
    /// The answer.
    pub answer: EscalationAnswer,
}

/// The lawyer dismissed a denial's card (thread stream): the card is hidden, nothing granted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DenialDismissed {
    /// The denial.
    pub denial: DenialId,
}

/// Too many refusals in a row, or in the turn: the host cancelled the turn (thread stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct DenialsPaused {
    /// The turn.
    pub turn: TurnId,
    /// The refusals counted toward the pause.
    pub refused: u16,
}

/// The matter remembers an answer (matter stream): applied at each launch, listed, forgettable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AllowanceRecorded {
    /// The remembered answer.
    pub allowance: AllowanceId,
    /// What it covers.
    pub target: AllowTarget,
    /// How far an allow reaches.
    pub mode: GrantMode,
    /// Allowed (`true`) or refused (`false`).
    pub allowed: bool,
}

/// The matter forgot a remembered answer (matter stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AllowanceForgotten {
    /// The remembered answer.
    pub allowance: AllowanceId,
}

/// Another matter's agents were allowed to read some of this matter's files (matter stream,
/// in the matter whose files are read). What was read is never logged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ReadGrantedTo {
    /// The matter whose agents may read.
    pub by_matter: MatterId,
    /// Its name when granted (display only, shown bidi-isolated).
    pub by_name: String,
    /// The allowance in `by_matter`'s stream.
    pub allowance: AllowanceId,
}

/// A read grant to another matter ended (matter stream, in the matter whose files were read).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ReadGrantForgotten {
    /// The matter whose agents could read.
    pub by_matter: MatterId,
    /// The allowance in `by_matter`'s stream.
    pub allowance: AllowanceId,
}

/// The lawyer switched the matter's protection off (`off: true`, after the dialog) or on
/// again (matter stream). Applies at the next launch; `NotProtectedCause::SwitchedOff` takes
/// its time from this event's envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ProtectionSwitched {
    /// Off, or on again.
    pub off: bool,
}
