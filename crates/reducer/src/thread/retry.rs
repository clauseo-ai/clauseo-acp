//! The agent's wait to retry its service on the header (`HeaderEntry::retrying`): `AgentRetrying`
//! sets it, `since` from the first of an unbroken wait and the words from the last;
//! `AgentRetryEnded`, the turn's end, a new turn's start and a detach clear it.

use clauseo_protocol::{AgentRetry, AgentRetrying, DomainEvent, Timestamp};

use super::{set, set_some, update_header};
use crate::Applied;
use crate::state::ThreadState;

/// The agent waits to retry: the header shows its words, from the wait's first `ts`.
pub(super) fn retrying(
    state: &mut ThreadState,
    ts: Timestamp,
    event: &AgentRetrying,
    out: &mut Applied,
) {
    update_header(state, "agent_retrying", out, |h| {
        let since = h.retrying.as_ref().map_or(ts, |retry| retry.since);
        let retry = AgentRetry {
            since,
            title: event.title.clone(),
            details: event.details.clone(),
        };
        set_some(&mut h.retrying, &retry)
    });
}

/// Before `event` folds: when it ends a wait (its end, the turn's end, a new turn's start, a
/// detach), the header's `retrying` goes, with one `Upsert` when it was set. With none set,
/// nothing happens, so a header never set stays as it was.
pub(super) fn ends(state: &mut ThreadState, event: &DomainEvent, out: &mut Applied) {
    let ends = matches!(
        event,
        DomainEvent::AgentRetryEnded(_)
            | DomainEvent::TurnStarted(_)
            | DomainEvent::TurnEnded(_)
            | DomainEvent::AgentSessionDetached(_)
    );
    let waiting = state
        .header
        .as_ref()
        .is_some_and(|header| header.retrying.is_some());
    if ends && waiting {
        // The header is there, so the name never reaches an anomaly.
        update_header(state, "retry_over", out, |h| set(&mut h.retrying, None));
    }
}
