//! The read triggers: when a focus, a turn's end, a "try again", a login change or an answered
//! question starts a read. At most one read runs per agent, and none while the agent asks a
//! question.

use clauseo_protocol::Timestamp;

use crate::kinds::kind_rules;
use crate::model::AgentState;
use crate::policy::Action;

/// A window shown again reads when the last read started at least 5 minutes ago.
const SHOWN_AFTER_MS: i64 = 5 * 60 * 1000;

/// A "try again" within this long of the last read's start starts no read, whatever window
/// sent it (the Settings window's loop must not launch one reader after another).
const TRY_AGAIN_AFTER_MS: i64 = 10_000;

/// Whether a read of one agent runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Flight {
    /// None runs.
    #[default]
    Idle,
    /// One runs.
    Reading,
    /// One runs, but the login changed since it started: read again once it ends.
    ReadingStale,
}

/// One agent's read policy state.
#[derive(Debug, Clone, Default)]
pub(crate) struct Reads {
    /// Whether a read runs.
    pub(crate) flight: Flight,
    /// When the last read started, whatever its outcome.
    pub(crate) last_read_started: Option<Timestamp>,
    /// The agent asks a question (a permission or an elicitation).
    pub(crate) asking: bool,
    /// A read was asked for while the agent asked a question.
    pub(crate) pending_read: bool,
    /// Whether a live signal came since the agent's last turn end. There is no turn-start input,
    /// so the last turn end stands in for this turn's start.
    turn_signal: TurnSignal,
}

impl Reads {
    /// A live signal came: the agent's next turn end waits the longer age.
    pub(crate) fn live_seen(&mut self) {
        self.turn_signal = TurnSignal::Live;
    }

    /// Whether a read is running.
    pub(crate) fn in_flight(&self) -> bool {
        self.flight != Flight::Idle
    }
}

/// Whether a live signal came during the agent's turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum TurnSignal {
    /// None came.
    #[default]
    None,
    /// At least one came.
    Live,
}

/// Asks for a read of `state`'s agent. The read in flight counts; while the agent asks a
/// question, the read waits for the answer.
pub(crate) fn request_read(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) {
    let reads = &mut state.reads;
    if reads.in_flight() {
        return;
    }
    if reads.asking {
        reads.pending_read = true;
        return;
    }
    reads.flight = Flight::Reading;
    reads.last_read_started = Some(now);
    actions.push(Action::Read(state.agent));
}

/// A window got focus, or a meter or the settings page opened.
pub(crate) fn shown(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) {
    let _ = read_if_older(state, SHOWN_AFTER_MS, now, actions);
}

/// A "try again" starts a read unless the last one started less than [`TRY_AGAIN_AFTER_MS`]
/// ago. Returns `true` when it starts none for that reason (the caller then re-publishes the
/// view). A read in flight is joined by [`request_read`] as before, and so is a read asked for
/// while a question is pending.
pub(crate) fn try_again(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) -> bool {
    !read_if_older(state, TRY_AGAIN_AFTER_MS, now, actions)
}

/// The login changed: read at once whatever the age; a read in flight read the old login, so
/// another follows it ([`Flight::ReadingStale`]).
pub(crate) fn login_changed(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) {
    if state.reads.in_flight() {
        state.reads.flight = Flight::ReadingStale;
        return;
    }
    request_read(state, now, actions);
}

/// A turn ended; `read_now` reads whatever the age, else the agent kind's age applies: the
/// longer one when a live signal came during the turn, the shorter one when none did. Either
/// way the next turn starts with no live signal seen.
pub(crate) fn turn_ended(
    state: &mut AgentState,
    read_now: bool,
    now: Timestamp,
    actions: &mut Vec<Action>,
) {
    let signal = std::mem::take(&mut state.reads.turn_signal);
    if read_now {
        request_read(state, now, actions);
        return;
    }
    let rules = kind_rules(state.agent);
    let min_age = match signal {
        TurnSignal::Live => rules.turn_read_after_ms,
        TurnSignal::None => rules.turn_read_after_ms_no_live,
    };
    let _ = read_if_older(state, min_age, now, actions);
}

/// A question became pending, or was answered: the read asked for meanwhile starts now.
pub(crate) fn asking(
    state: &mut AgentState,
    asking: bool,
    now: Timestamp,
    actions: &mut Vec<Action>,
) {
    state.reads.asking = asking;
    if !asking && state.reads.pending_read {
        state.reads.pending_read = false;
        request_read(state, now, actions);
    }
}

/// Asks for a read when the last one started at least `min_age_ms` ago, or never. Returns
/// whether the age allowed it, whether or not [`request_read`] then started one.
fn read_if_older(
    state: &mut AgentState,
    min_age_ms: i64,
    now: Timestamp,
    actions: &mut Vec<Action>,
) -> bool {
    let due = state
        .reads
        .last_read_started
        .is_none_or(|started| now.as_millis().saturating_sub(started.as_millis()) >= min_age_ms);
    if due {
        request_read(state, now, actions);
    }
    due
}
