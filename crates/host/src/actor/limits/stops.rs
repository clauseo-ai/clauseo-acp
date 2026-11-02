//! Pending stops: a turn stopped at the limit with no reset time logs its `TurnStoppedAtLimit`
//! after its agent's next read, as a commit of its own.
//!
//! - The stop is decided by the agent's next `ReadDone`, whichever read it is, from
//!   [`Limits::at_limit`](clauseo_limits::Limits::at_limit) (nothing after a failed read), or
//!   after 10 s on the host clock with no `ReadDone`, from `at_limit` as it stands then.
//! - It is logged only once the commit holding the turn's `TurnEnded` is durable: while the
//!   thread's closing record for that turn is there, it waits, and the record's `Ok` logs it.
//!   A turn whose end never becomes durable gets no stop in this run.
//! - It stays until its own commit returns `Ok`, and keeps the thread's stream loaded
//!   meanwhile. A blocked stream retries it at the unblock, a refused or failed commit at the
//!   next unblock (twice at most), then at the thread's next durable end or the agent's next
//!   read. One lost to a restart is logged by start-up repair (`turn/reconcile.rs`).

use clauseo_protocol::{
    AgentKind, LimitStopReason, StreamId, ThreadId, TurnId, TurnStoppedAtLimit,
};
use futures::future::AbortHandle;

use super::super::Actor;
use super::super::input::Input;
use super::super::turn;
use super::{LimitsInput, at_limit, turn_ended};

/// How many failed commits of one stop the unblock retries.
const UNBLOCK_RETRIES: u8 = 2;

/// A turn stopped at the limit with no reset time, until the commit logging its stop is `Ok`.
pub(super) struct PendingStop {
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    /// From the turn's limit signal.
    reason: LimitStopReason,
    /// The stop, once a read or the 10 s bound decided it.
    stopped: Option<TurnStoppedAtLimit>,
    /// The 10 s bound, while no read has decided it.
    timer: Option<AbortHandle>,
    /// The commit logging it is in flight.
    sending: bool,
    /// Commits of it that failed or were refused.
    failed: u8,
}

/// A turn of `thread` stopped at the limit with no reset time: its `TurnStoppedAtLimit` waits
/// for `agent`'s next read, which this asks for (`TurnEnded { read_now: true }`), or 10 s.
pub(crate) fn stop_later(
    actor: &mut Actor,
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    reason: LimitStopReason,
) {
    let wait = Input::Limits(LimitsInput::StopWait { thread, turn });
    let timer = actor.post_after(turn::LIMIT_READ_WAIT, wait);
    actor.limits.stops.push(PendingStop {
        agent,
        thread,
        turn,
        reason,
        stopped: None,
        timer: Some(timer),
        sending: false,
        failed: 0,
    });
    turn_ended(actor, agent, true);
}

/// A read of `agent` is done and the model has its outcome: each of the agent's stops not yet
/// decided takes the window and reset time `at_limit` gives (none after a failed read), and
/// every decided stop of the agent not logged yet is tried.
pub(super) fn read_done(actor: &mut Actor, agent: AgentKind, failed: bool) {
    let hit = if failed { None } else { at_limit(actor, agent) };
    let mut threads = Vec::new();
    for stop in actor
        .limits
        .stops
        .iter_mut()
        .filter(|stop| stop.agent == agent)
    {
        if stop.stopped.is_none() {
            stop.decide(hit.as_ref());
        }
        if !threads.contains(&stop.thread) {
            threads.push(stop.thread);
        }
    }
    for thread in threads {
        log_ready(actor, thread, false);
    }
}

/// The 10 s bound of `turn`'s stop is over with no read: `at_limit` as it stands decides it.
pub(super) fn on_wait_over(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(agent) = actor
        .limits
        .stops
        .iter()
        .find(|stop| stop.turn == turn && stop.stopped.is_none())
        .map(|stop| stop.agent)
    else {
        return;
    };
    let hit = at_limit(actor, agent);
    if let Some(stop) = actor.limits.stops.iter_mut().find(|stop| stop.turn == turn) {
        stop.decide(hit.as_ref());
    }
    log_ready(actor, thread, false);
}

/// `thread`'s last end is durable, or its stream drained after a failure (`unblocked`): its
/// decided stops not logged yet are tried.
pub(crate) fn retry(actor: &mut Actor, thread: ThreadId, unblocked: bool) {
    log_ready(actor, thread, unblocked);
}

/// The commit logging `turn`'s stop returned: `Ok` forgets the stop, a failure leaves it for
/// a retry.
pub(crate) fn committed(actor: &mut Actor, thread: ThreadId, turn: TurnId, ok: bool) {
    let stops = &mut actor.limits.stops;
    let Some(index) = stops.iter().position(|stop| stop.turn == turn) else {
        return;
    };
    if ok {
        stops.remove(index);
        actor.release(StreamId::Thread(thread));
    } else if let Some(stop) = stops.get_mut(index) {
        stop.sending = false;
        stop.failed = stop.failed.saturating_add(1);
    }
}

/// Whether a stop of `thread` waits to be logged, which keeps its stream loaded.
pub(crate) fn holds(actor: &Actor, thread: ThreadId) -> bool {
    actor.limits.stops.iter().any(|stop| stop.thread == thread)
}

impl PendingStop {
    /// The stop, from `hit` (both `None` without one); the 10 s bound is no longer needed.
    fn decide(&mut self, hit: Option<&clauseo_limits::LimitHit>) {
        self.stopped = Some(TurnStoppedAtLimit {
            turn: self.turn,
            window: hit.and_then(|hit| hit.window.clone()),
            resets_at: hit.and_then(|hit| hit.resets_at),
            reason: self.reason,
        });
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
    }
}

/// Logs each decided stop of `thread` whose commit is not in flight, unless its turn's end is
/// not durable yet. At an unblock, a stop whose commit failed [`UNBLOCK_RETRIES`] times waits.
fn log_ready(actor: &mut Actor, thread: ThreadId, unblocked: bool) {
    let ready: Vec<TurnStoppedAtLimit> = actor
        .limits
        .stops
        .iter()
        .filter(|stop| stop.thread == thread && !stop.sending)
        .filter(|stop| !unblocked || stop.failed < UNBLOCK_RETRIES)
        .filter_map(|stop| stop.stopped.clone())
        .collect();
    for stopped in ready {
        let turn = stopped.turn;
        if turn::closing(actor, thread, turn) {
            continue;
        }
        if actor.loaded(StreamId::Thread(thread)).is_none() {
            reload(actor, thread);
            return;
        }
        let sent = turn::log_limit_stop(actor, thread, stopped);
        if let Err(error) = &sent {
            tracing::warn!(%thread, %turn, %error, "a stop at the limit was not logged yet");
        }
        let blocked = turn::stream_blocked(actor, thread);
        if let Some(stop) = actor.limits.stops.iter_mut().find(|stop| stop.turn == turn) {
            match sent {
                Ok(()) => stop.sending = true,
                // A blocked stream retries it at the unblock without counting.
                Err(_) if !blocked => stop.failed = stop.failed.saturating_add(1),
                Err(_) => {}
            }
        }
    }
}

/// Loads `thread`'s stream again, then tries its stops.
fn reload(actor: &mut Actor, thread: ThreadId) {
    let stream = StreamId::Thread(thread);
    super::super::streams::with_loaded(
        actor,
        stream,
        Box::new(move |actor, loaded| match loaded {
            Ok(()) => log_ready(actor, thread, false),
            Err(error) => {
                tracing::warn!(%thread, %error, "a stop at the limit waits: its thread did not load");
            }
        }),
    );
}
