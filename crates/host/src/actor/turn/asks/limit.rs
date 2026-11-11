//! The agent's limit signals (`asks.rs` has the rest of its asks): a rejection or a stop marks
//! the live turn (the first wins); a rate limit also feeds the limits model.

use clauseo_acp::{LimitSignal, LimitStatus, Origin};
use clauseo_protocol::{AgentSessionId, LimitStopReason, ThreadId};

use super::super::super::{Actor, limits};
use super::super::{LimitMark, current};

/// A limit signal: a rejection or a stop during the turn marks it (the first wins); anything
/// else goes to `tracing`. A rate limit, in a turn or outside one, also goes to the limits
/// model as a live signal, unless it is history replayed by `session/load`.
pub(in super::super) fn on_limit(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    signal: &LimitSignal,
) {
    if !matches!(origin, Origin::Replay) {
        limits::live(actor, thread, signal);
    }
    let mark = limit_mark(signal);
    let turn = match origin {
        Origin::Turn(turn) => Some(turn),
        Origin::Late(_) | Origin::Replay | Origin::Idle => None,
    };
    if let Some(turn) = turn {
        super::super::offline::agent_active(actor, thread, turn);
    }
    let current = current(actor, thread).filter(|current| Some(current.turn) == turn);
    if let (Some(current), Some(mark)) = (current, mark) {
        if current.limit.is_none() {
            current.limit = Some(mark);
        }
    } else {
        tracing::info!(%thread, %session, ?signal, "a limit signal");
    }
}

/// The limit stop a signal means, if it stops the turn.
pub(in super::super) fn limit_mark(signal: &LimitSignal) -> Option<LimitMark> {
    match signal {
        LimitSignal::RateLimit {
            window,
            status,
            resets_at,
            ..
        } => match status {
            LimitStatus::Rejected => Some(LimitMark {
                window: window.clone(),
                resets_at: *resets_at,
                reason: LimitStopReason::PlanWindow,
            }),
            LimitStatus::Allowed | LimitStatus::Warning => None,
        },
        LimitSignal::Stopped { reason, .. } => Some(LimitMark {
            window: None,
            resets_at: None,
            reason: *reason,
        }),
    }
}
