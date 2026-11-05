//! Pending stops: a turn stopped at the limit with no reset time logs its `TurnStoppedAtLimit`
//! after its agent's next read, as a commit of its own.
//!
//! - The stop is decided by the `ReadDone` of the agent's next read started after it, from
//!   [`Limits::at_limit`](clauseo_limits::Limits::at_limit) (nothing after a failed read), or,
//!   with none within [`read_wait`] of the agent's kind on the host clock, from `at_limit` as
//!   it stands then. A read already running when the stop came may have begun before the
//!   limit: its `ReadDone` decides nothing and asks for a read again (`TurnEnded { read_now:
//!   true }`, so the model's one read per agent still holds).
//! - Shutdown decides every stop still waiting from `at_limit` as it stands, and one that comes
//!   during shutdown is decided at once, with no read. The actor stays up until each commit
//!   logging one has returned ([`busy`]); a stop whose turn's end is not durable yet is logged
//!   by that end's `Ok`, as at any time.
//! - It is logged only once the commit holding the turn's `TurnEnded` is durable: while the
//!   thread's closing record for that turn is there, it waits, and the record's `Ok` logs it.
//!   A turn whose end never becomes durable gets no stop in this run.
//! - It stays until its own commit returns `Ok`, and keeps the thread's stream loaded
//!   meanwhile. A blocked stream retries it at the unblock, a refused or failed commit at the
//!   next unblock (twice at most), then at the thread's next durable end or the agent's next
//!   read. One lost to a crash is logged by start-up repair (`turn/reconcile.rs`).

use clauseo_protocol::{
    AgentKind, LimitStopReason, StreamId, ThreadId, TurnId, TurnStoppedAtLimit,
};
use futures::future::AbortHandle;

use super::super::Actor;
use super::super::input::Input;
use super::super::state::HostPhase;
use super::super::turn;
use super::{LimitsInput, at_limit, read_wait, turn_ended};
use crate::log_file::Cause;

/// How many failed commits of one stop the unblock retries.
const UNBLOCK_RETRIES: u8 = 2;

/// A turn stopped at the limit with no reset time, until the commit logging its stop is `Ok`.
pub(super) struct PendingStop {
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    /// From the turn's limit signal.
    reason: LimitStopReason,
    /// The number of the first read that may decide it: the next one started after the stop.
    first_read: u64,
    /// The stop, once a read, the wait or shutdown decided it.
    stopped: Option<TurnStoppedAtLimit>,
    /// The wait for a read, while nothing has decided it.
    timer: Option<AbortHandle>,
    /// The commit logging it is in flight.
    sending: bool,
    /// Commits of it that failed or were refused.
    failed: u8,
}

/// A turn of `thread` stopped at the limit with no reset time: its `TurnStoppedAtLimit` waits
/// for `agent`'s next read, which this asks for (`TurnEnded { read_now: true }`), or
/// [`read_wait`]. During shutdown `at_limit` decides it now; the end commit, sent after this
/// call, logs it once it is `Ok`.
pub(crate) fn stop_later(
    actor: &mut Actor,
    agent: AgentKind,
    thread: ThreadId,
    turn: TurnId,
    reason: LimitStopReason,
) {
    let mut stop = PendingStop {
        agent,
        thread,
        turn,
        reason,
        first_read: actor.limits.next_read,
        stopped: None,
        timer: None,
        sending: false,
        failed: 0,
    };
    if matches!(actor.phase, HostPhase::Stopping(_)) {
        stop.decide(at_limit(actor, agent).as_ref());
        actor.limits.stops.push(stop);
        return;
    }
    let wait = Input::Limits(LimitsInput::StopWait { thread, turn });
    stop.timer = Some(actor.post_after(read_wait(agent), wait));
    actor.limits.stops.push(stop);
    turn_ended(actor, agent, true);
}

/// Shutdown: each stop nothing has decided yet takes the window and reset time `at_limit`
/// gives now, and every stop not logged yet is tried (one whose turn's end is not durable yet
/// waits for that end's `Ok`).
pub(crate) fn decide_now(actor: &mut Actor) {
    for agent in [AgentKind::Claude, AgentKind::Codex] {
        let hit = at_limit(actor, agent);
        for stop in actor
            .limits
            .stops
            .iter_mut()
            .filter(|stop| stop.agent == agent && stop.stopped.is_none())
        {
            stop.decide(hit.as_ref());
        }
    }
    let mut threads = Vec::new();
    for stop in &actor.limits.stops {
        if !threads.contains(&stop.thread) {
            threads.push(stop.thread);
        }
    }
    for thread in threads {
        log_ready(actor, thread, false);
    }
}

/// Whether shutdown waits for a stop: one not decided yet, one whose commit is in flight, or a
/// decided one whose stream is blocked while the unblock will still retry it (a stop whose
/// stream is not blocked gets no retry, so shutdown never waits for it).
pub(crate) fn busy(actor: &Actor) -> bool {
    actor.limits.stops.iter().any(|stop| {
        stop.stopped.is_none()
            || stop.sending
            || (stop.failed > 0
                && stop.failed < UNBLOCK_RETRIES
                && turn::stream_blocked(actor, stop.thread))
    })
}

/// Read `number` of `agent` is done and the model has its outcome: each of the agent's stops
/// not yet decided that this read may decide takes the window and reset time `at_limit` gives
/// (none after a failed read), and every decided stop of the agent not logged yet is tried. A
/// stop that came while this read ran asks for another read.
pub(super) fn read_done(actor: &mut Actor, agent: AgentKind, failed: bool, number: u64) {
    let hit = if failed { None } else { at_limit(actor, agent) };
    let mut threads = Vec::new();
    let mut again = false;
    for stop in actor
        .limits
        .stops
        .iter_mut()
        .filter(|stop| stop.agent == agent)
    {
        if stop.stopped.is_none() {
            if number < stop.first_read {
                again = true;
            } else {
                stop.decide(hit.as_ref());
            }
        }
        if !threads.contains(&stop.thread) {
            threads.push(stop.thread);
        }
    }
    for thread in threads {
        log_ready(actor, thread, false);
    }
    if again {
        turn_ended(actor, agent, true);
    }
}

/// The wait of `turn`'s stop for a read is over: `at_limit` as it stands decides it.
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
    /// The stop, from `hit` (both `None` without one); the wait is no longer needed.
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
            tracing::warn!(
                %thread,
                %turn,
                error = %Cause(error),
                "a stop at the limit was not logged yet"
            );
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
                tracing::warn!(
                    %thread,
                    error = %Cause(&error),
                    "a stop at the limit waits: its thread did not load"
                );
            }
        }),
    );
}
