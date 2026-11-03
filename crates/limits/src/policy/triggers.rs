//! The read triggers: when a focus, a turn's end, a "try again" or an answered question starts
//! a read. At most one read runs per agent, and none while the agent asks a question.

use clauseo_protocol::Timestamp;

use crate::kinds::kind_rules;
use crate::model::AgentState;
use crate::policy::Action;

/// A window shown again reads when the last read started at least 5 minutes ago.
const SHOWN_AFTER_MS: i64 = 5 * 60 * 1000;

/// A "try again" within this long of the last read's start starts no read, whatever window
/// sent it (the Settings window's loop must not launch one reader after another).
const TRY_AGAIN_AFTER_MS: i64 = 10_000;

/// One agent's read policy state.
#[derive(Debug, Clone, Default)]
pub(crate) struct Reads {
    /// A read is running.
    pub(crate) in_flight: bool,
    /// When the last read started, whatever its outcome.
    pub(crate) last_read_started: Option<Timestamp>,
    /// The agent asks a question (a permission or an elicitation).
    pub(crate) asking: bool,
    /// A read was asked for while the agent asked a question.
    pub(crate) pending_read: bool,
}

/// Asks for a read of `state`'s agent. The read in flight counts; while the agent asks a
/// question, the read waits for the answer.
pub(crate) fn request_read(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) {
    let reads = &mut state.reads;
    if reads.in_flight {
        return;
    }
    if reads.asking {
        reads.pending_read = true;
        return;
    }
    reads.in_flight = true;
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

/// A turn ended; `read_now` reads whatever the age, else the agent kind's age applies.
pub(crate) fn turn_ended(
    state: &mut AgentState,
    read_now: bool,
    now: Timestamp,
    actions: &mut Vec<Action>,
) {
    if read_now {
        request_read(state, now, actions);
        return;
    }
    let min_age = kind_rules(state.agent).turn_read_after_ms;
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
