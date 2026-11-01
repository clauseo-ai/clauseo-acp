//! The limits model's state per account, and the merge of reads and live signals into it.
//!
//! [`AgentState`] holds one agent's account data (exactly an `AccountLimits` once the account is
//! known) beside flags that are never stored, so a restart forgets them: which rows were
//! rejected, which passed their reset time, which got their stale mark, each row's last update,
//! whether the usage method is missing, and the limit the vendor says the account reached.

mod hit;
mod merge;

use clauseo_protocol::{
    AccountLimits, AccountRef, AgentKind, AtLimitBehaviour, Billing, Finite, LimitSource,
    LimitStopReason, LimitWindow, PlanLabel, StaleReason, Timestamp, WindowKind,
};

use crate::policy::Reads;

pub(crate) use hit::at_limit;
pub(crate) use merge::{apply_live, apply_read};

/// How long a row's numbers stay fresh: 15 minutes, in milliseconds.
pub(crate) const STALE_AFTER_MS: i64 = 15 * 60 * 1000;

/// A live rate-limit signal, field for field as `acp::LimitSignal::RateLimit`: the host turns
/// one into the other.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSignal {
    /// The window's id (`rateLimitType`), such as `five_hour`.
    pub window: Option<String>,
    /// The agent's status for it.
    pub status: LiveStatus,
    /// How much of it is used, 0 to 100.
    pub used_percent: Option<Finite>,
    /// When it resets.
    pub resets_at: Option<Timestamp>,
}

/// The status of a live signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveStatus {
    /// `allowed`.
    Allowed,
    /// `allowed_warning`: close to the limit.
    Warning,
    /// `rejected`: at the limit.
    Rejected,
}

/// Which limit an agent is at, for the host's at-limit stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitHit {
    /// The window's id, when one window is at its limit.
    pub window: Option<String>,
    /// When that window resets.
    pub resets_at: Option<Timestamp>,
    /// Which limit it is.
    pub reason: LimitStopReason,
}

/// One window with the model-only flags that never reach `AccountLimits`.
#[derive(Debug, Clone)]
pub(crate) struct Row {
    /// The window, as the agent last sent it.
    pub(crate) window: LimitWindow,
    /// When a read or a live signal last changed it.
    pub(crate) updated_at: Timestamp,
    /// A live signal said `rejected`. A good read, a later `allowed` or `allowed_warning` for
    /// the window, or its passed reset clears it.
    pub(crate) rejected: bool,
    /// Its reset time passed: its old percentage is no longer shown.
    pub(crate) reset_passed: bool,
    /// Its 15-minute stale mark was acted on.
    pub(crate) stale_noted: bool,
}

impl Row {
    /// A row read or changed at `now`, with no flag set.
    pub(crate) fn new(window: LimitWindow, now: Timestamp) -> Row {
        Row {
            window,
            updated_at: now,
            rejected: false,
            reset_passed: false,
            stale_noted: false,
        }
    }

    /// When its numbers turn stale, in milliseconds.
    pub(crate) fn stale_at_ms(&self) -> i64 {
        self.updated_at.as_millis().saturating_add(STALE_AFTER_MS)
    }
}

/// One agent's state: its account data, the model-only flags and the read policy's state.
#[derive(Debug, Clone)]
pub(crate) struct AgentState {
    /// The agent.
    pub(crate) agent: AgentKind,
    /// The account; `None` until a cache entry or a good read names it.
    pub(crate) account: Option<AccountRef>,
    /// The plan's name.
    pub(crate) plan: Option<PlanLabel>,
    /// How the account pays.
    pub(crate) billing: Billing,
    /// The windows, sorted: session, weekly, model, other.
    pub(crate) rows: Vec<Row>,
    /// What happens at the limit.
    pub(crate) at_limit: AtLimitBehaviour,
    /// How fresh the numbers are.
    pub(crate) source: LimitSource,
    /// The last read said the usage method is missing (Claude only).
    pub(crate) method_missing: bool,
    /// The limit the last good read says the account reached.
    pub(crate) reached: Option<LimitStopReason>,
    /// The read policy's state.
    pub(crate) reads: Reads,
}

impl AgentState {
    /// `agent` with no account yet: its first read is about to start.
    pub(crate) fn loading(agent: AgentKind) -> AgentState {
        AgentState {
            agent,
            account: None,
            plan: None,
            billing: Billing::Plan,
            rows: Vec::new(),
            at_limit: AtLimitBehaviour::Pauses,
            source: LimitSource::Loading,
            method_missing: false,
            reached: None,
            reads: Reads::default(),
        }
    }

    /// `agent` from its cached limits, already stale since a restart passed. `Loading` and
    /// `Unknown` caches count as none.
    pub(crate) fn from_cache(
        agent: AgentKind,
        cached: AccountLimits,
        now: Timestamp,
    ) -> AgentState {
        let (source, updated_at) = match cached.source {
            LimitSource::Fresh { checked_at } | LimitSource::Stale { checked_at, .. } => (
                LimitSource::Stale {
                    checked_at,
                    reason: StaleReason::Aged,
                },
                checked_at,
            ),
            LimitSource::LiveOnly { updated_at } => (
                LimitSource::Stale {
                    checked_at: updated_at,
                    reason: StaleReason::Aged,
                },
                updated_at,
            ),
            source @ (LimitSource::NotApplicable | LimitSource::Unavailable { .. }) => {
                (source, now)
            }
            LimitSource::Loading | LimitSource::Unknown { .. } => {
                return AgentState::loading(agent);
            }
        };
        let rows = cached
            .windows
            .into_iter()
            .map(|window| Row {
                stale_noted: true,
                ..Row::new(window, updated_at)
            })
            .collect();
        AgentState {
            agent,
            account: Some(cached.account),
            plan: cached.plan,
            billing: cached.billing,
            rows,
            at_limit: cached.at_limit,
            source,
            method_missing: false,
            reached: None,
            reads: Reads::default(),
        }
    }

    /// The limits as now held, once the account is known.
    pub(crate) fn limits(&self) -> Option<AccountLimits> {
        let account = self.account.clone()?;
        Some(AccountLimits {
            agent: self.agent,
            account,
            plan: self.plan.clone(),
            billing: self.billing,
            windows: self.rows.iter().map(|row| row.window.clone()).collect(),
            at_limit: self.at_limit,
            source: self.source.clone(),
        })
    }

    /// The rows the view shows: none for a source without bars (`NotApplicable`, `Loading`,
    /// `Unavailable`). Deadlines and `at_limit` look at these rows only.
    pub(crate) fn visible_rows(&self) -> &[Row] {
        if shows_rows(&self.source) {
            &self.rows
        } else {
            &[]
        }
    }

    /// The visible rows, to change their flags.
    pub(crate) fn visible_rows_mut(&mut self) -> &mut [Row] {
        if shows_rows(&self.source) {
            &mut self.rows
        } else {
            &mut []
        }
    }
}

/// Whether a source shows bars.
fn shows_rows(source: &LimitSource) -> bool {
    !matches!(
        source,
        LimitSource::NotApplicable | LimitSource::Loading | LimitSource::Unavailable { .. }
    )
}

/// The sort rank of a window kind: session, weekly, model, other.
pub(crate) fn kind_rank(kind: WindowKind) -> u8 {
    match kind {
        WindowKind::Session => 0,
        WindowKind::Weekly => 1,
        WindowKind::Model => 2,
        WindowKind::Other => 3,
    }
}
