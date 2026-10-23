//! Account limits: the usage windows of the lawyer's agent plan, why a turn stopped at a limit,
//! and the view the UI renders from them.
//!
//! `AccountLimits` travels as a host event and is never logged; `store` keeps the last snapshot
//! per account. `TurnStoppedAtLimit` (in `event/payload/turn.rs`) is logged in its thread, so the
//! at-limit card survives a reopen. A limit is not an error: there is no `ErrorCode` for it.
//! Mapping `AccountLimits` to [`LimitsView`] needs the clock, so it belongs to the `limits`
//! crate, with `now` as input.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::open_enum::{open_data_enum, open_unit_enum};
use crate::{Finite, Timestamp};

/// The agent a limit belongs to. Only for limits; threads name agents by [`AgentId`].
///
/// [`AgentId`]: crate::AgentId
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AgentKind {
    /// Claude Code.
    Claude,
    /// Codex.
    Codex,
}

/// An opaque account id from the agent's auth status. Never a token: nothing here can tell one
/// apart, so the `limits` crate keeps tokens out when it fills this.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AccountRef(pub String);

/// The plan's name as the agent reports it, such as "Claude Max".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PlanLabel(pub String);

/// One account's limits, as `limits` last read them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AccountLimits {
    /// The agent.
    pub agent: AgentKind,
    /// The account.
    pub account: AccountRef,
    /// The plan's name, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plan: Option<PlanLabel>,
    /// How the account pays.
    pub billing: Billing,
    /// The windows, sorted: session, weekly, then model-scoped.
    pub windows: Vec<LimitWindow>,
    /// What happens at the limit.
    pub at_limit: AtLimitBehaviour,
    /// How fresh these numbers are.
    pub source: LimitSource,
}

/// How an account pays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Billing {
    /// A subscription plan with usage windows.
    Plan,
    /// Pay as you go, billed by a provider; no windows apply.
    PayAsYouGo {
        /// Who bills it.
        via: BillingProvider,
    },
}

/// Who bills a pay-as-you-go account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BillingProvider {
    /// An Anthropic API key.
    Anthropic,
    /// AWS Bedrock.
    Bedrock,
    /// Google Cloud Vertex.
    Vertex,
    /// An `OpenAI` API key.
    OpenAi,
    /// Anyone else.
    Other,
}

/// One usage window, such as the 5-hour session or the week.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LimitWindow {
    /// The window's id, as the agent names it.
    pub id: String,
    /// What it counts.
    pub kind: WindowKind,
    /// A label, such as "5-hour limit".
    pub label: String,
    /// How much of it is used, 0 to 100.
    pub used_percent: Finite,
    /// When it resets, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resets_at: Option<Timestamp>,
    /// Its length in minutes, when known.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub duration_mins: Option<u32>,
    /// The agent warned that the limit is near (Claude's `allowed_warning`).
    pub warning: bool,
}

/// What a usage window counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum WindowKind {
    /// The rolling session window (Claude's 5 hours, Codex's primary window).
    Session,
    /// The weekly window.
    Weekly,
    /// A window for one model only, such as a weekly limit on the largest model.
    Model,
    /// A window `limits` does not classify; its label says what it is.
    Other,
}

/// What happens when the account reaches a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum AtLimitBehaviour {
    /// The agent pauses until the window resets.
    Pauses,
    /// Claude continues on extra usage, billed to the account, when enabled.
    ExtraUsage {
        /// Extra usage is turned on.
        enabled: bool,
    },
    /// Codex continues on the plan's credits, when there are any.
    Credits {
        /// The plan has credits left.
        has_credits: bool,
    },
}

open_data_enum! {
    /// How fresh an account's limits are.
    ///
    /// A plain string for a unit variant, a one-key object for the others; a kind from a newer
    /// Clauseo decodes to `Unknown`.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum LimitSource {
        /// Read less than 15 minutes ago.
        Fresh = "fresh" {
            /// When it was read.
            checked_at: Timestamp,
        },
        /// The last good read is older, or the last refresh failed; the numbers show muted.
        Stale = "stale" {
            /// When the last good read was.
            checked_at: Timestamp,
            /// Why it is stale.
            reason: StaleReason,
        },
        /// The first read has not finished.
        Loading = "loading",
        /// No numbers are available.
        Unavailable = "unavailable" {
            /// Why.
            reason: UnavailableReason,
        },
        /// The account pays as it goes; no windows apply.
        NotApplicable = "not_applicable",
        /// The plan read is gone (the experimental Claude call was removed or reshaped); the
        /// bars come only from the agent's live `_claude/rateLimit` signals during a turn.
        LiveOnly = "live_only" {
            /// When the last live signal arrived.
            updated_at: Timestamp,
        },
        _ =>
        /// A state from a newer Clauseo; written back as the plain string `kind`.
        Unknown,
    }
    test mod limit_source with [
        LimitSource::Fresh {
            checked_at: Timestamp::ZERO,
        },
        LimitSource::Stale {
            checked_at: Timestamp::ZERO,
            reason: StaleReason::ReadFailed,
        },
        LimitSource::Loading,
        LimitSource::Unavailable {
            reason: UnavailableReason::MissingScope,
        },
        LimitSource::NotApplicable,
        LimitSource::LiveOnly {
            updated_at: Timestamp::ZERO,
        },
    ];
}

/// Why limits are stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum StaleReason {
    /// The last good read is 15 minutes old or more.
    Aged,
    /// The last refresh failed.
    ReadFailed,
}

/// Why no limits are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum UnavailableReason {
    /// Never read yet, and no read is running.
    NeverRead,
    /// Every read so far failed.
    ReadFailed,
    /// The sign-in lacks the scope that shows plan limits; signing in again may fix it.
    MissingScope,
}

open_unit_enum! {
    /// Which limit stopped a turn; a reason from a newer Clauseo decodes to `Unknown`.
    pub enum LimitStopReason {
        /// A plan window is used up.
        PlanWindow,
        /// The workspace has no credits left.
        WorkspaceCredits,
        /// The workspace reached its spend cap.
        WorkspaceSpendCap,
        /// A temporary rate limit.
        Temporary,
    }
    test mod limit_stop_reason;
}

/// What the UI shows for each signed-in agent: a JSON object keyed by agent kind.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LimitsView(pub BTreeMap<AgentKind, AgentLimitsView>);

/// One agent's entry in [`LimitsView`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AgentLimitsView {
    /// The plan's name.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub plan: Option<String>,
    /// Plan or pay as you go.
    pub billing: BillingKind,
    /// The composer's gauge: the tightest window; `None` when there is none to show.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub meter: Option<LimitMeter>,
    /// Every window, for the popover and the settings page.
    pub windows: Vec<LimitWindowView>,
    /// What happens at the limit, in the words `limits` chose.
    pub at_limit_text: String,
    /// How fresh the numbers are.
    pub freshness: Freshness,
    /// When they were read (for "Updated 25 min ago").
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub checked_at: Option<Timestamp>,
}

/// Plan or pay as you go, for the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BillingKind {
    /// A subscription plan.
    Plan,
    /// Pay as you go.
    PayAsYouGo,
}

/// The composer's gauge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LimitMeter {
    /// Its colour.
    pub tone: MeterTone,
    /// How much of the window is used, 0 to 100.
    pub used_percent: Finite,
    /// The window's label.
    pub window_label: String,
    /// When it resets.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resets_at: Option<Timestamp>,
}

/// A gauge's colour. Never red: a limit is not an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MeterTone {
    /// Below 75% used.
    Neutral,
    /// From 75%, or when the agent warns.
    Near,
    /// At 100%, or when the agent rejected a request.
    AtLimit,
}

/// How fresh an agent's numbers are, for the view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Freshness {
    /// Read less than 15 minutes ago.
    Fresh,
    /// Older, or the last refresh failed: same numbers, muted.
    Stale,
    /// First read running: grey bars, no numbers.
    Loading,
    /// Never available.
    Unavailable,
    /// The sign-in lacks the scope that shows plan limits.
    MissingScope,
    /// Pay as you go: no bars.
    NotApplicable,
    /// Bars from live signals only, while the agent works.
    LiveOnly,
}

/// One window's row in the popover and on the settings page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LimitWindowView {
    /// The window's id, matching [`LimitWindow::id`].
    pub id: String,
    /// What it counts.
    pub kind: WindowKind,
    /// Its label.
    pub label: String,
    /// How much is used, 0 to 100; `None` while loading or once its reset time passed, so an
    /// old percentage is never shown as current.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub used_percent: Option<Finite>,
    /// Its bar's colour.
    pub tone: MeterTone,
    /// When it resets.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub resets_at: Option<Timestamp>,
    /// How fresh this row is.
    pub freshness: WindowFreshness,
}

/// How fresh one window's row is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum WindowFreshness {
    /// Current numbers.
    Fresh,
    /// The same numbers, muted.
    Stale,
    /// Grey bar, no numbers.
    Loading,
    /// Its reset time passed and a refresh is running ("Reset at 4:10 pm. Refreshing...").
    ResetPassed,
}
