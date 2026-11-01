//! The deadlines: each row's 15-minute stale mark and each shown reset time. The model starts
//! no timer: the host sends `Input::Deadline` at [`next_deadline`].

use clauseo_protocol::Timestamp;

use crate::model::AgentState;
use crate::policy::Action;
use crate::policy::triggers::request_read;

/// Acts on every mark of `state` that passed by `now`: a row past its reset time shows no
/// percentage and asks one read; a row past its stale mark is noted. Returns whether the view
/// changed.
///
/// A reset that passed before the last read started asks no read: that read came after it. This
/// keeps a vendor that reports a past reset time from starting a read after every read.
pub(crate) fn sweep(state: &mut AgentState, now: Timestamp, actions: &mut Vec<Action>) -> bool {
    let last_started = state.reads.last_read_started;
    let mut changed = false;
    let mut read = false;
    for row in state.visible_rows_mut() {
        if let Some(resets_at) = row.window.resets_at
            && !row.reset_passed
            && resets_at <= now
        {
            row.reset_passed = true;
            changed = true;
            read |= last_started.is_none_or(|started| started < resets_at);
        }
        if !row.stale_noted && now.as_millis() >= row.stale_at_ms() {
            row.stale_noted = true;
            changed = true;
        }
    }
    if read {
        request_read(state, now, actions);
    }
    changed
}

/// The earliest mark not yet acted on, over every agent.
pub(crate) fn next_deadline(states: &[AgentState]) -> Option<Timestamp> {
    states
        .iter()
        .flat_map(AgentState::visible_rows)
        .flat_map(|row| {
            let stale = (!row.stale_noted)
                .then(|| Timestamp::new(row.stale_at_ms()).ok())
                .flatten();
            let reset = row.window.resets_at.filter(|_| !row.reset_passed);
            [stale, reset]
        })
        .flatten()
        .min()
}

/// The earliest reset time of `state` not yet passed.
pub(crate) fn next_read_at(state: &AgentState) -> Option<Timestamp> {
    state
        .visible_rows()
        .iter()
        .filter(|row| !row.reset_passed)
        .filter_map(|row| row.window.resets_at)
        .min()
}
