//! Step 6, the serving loop, with no Windows call: once per [`TICK`] it takes what the consumer
//! produced, sends each refusal the rate admits, sends what was dropped once a second, polls
//! the clients and steps the [`Lifetime`] and the [`Breaker`]. Every [`OWNER_CHECK`] it reads
//! the record again.

use std::time::Duration;

use super::breaker::{Action, Breaker, PAUSE};
use super::lifetime::{Lifetime, Verdict};
use super::protocol::{RateLimit, denied_line, dropped_line, paused_line, resumed_line};
use super::{Delivery, Pool, Refusal, WatchExit, Watcher};

/// How long one wait for the consumer lasts: the loop's tick.
pub(super) const TICK: Duration = Duration::from_millis(250);
/// How often the record's `OwnerSid` is read again.
pub(super) const OWNER_CHECK: Duration = Duration::from_secs(5);

/// Why the loop stopped serving, short of a failure.
enum Stop {
    Life(Verdict),
    /// The record names another owner, or none.
    OwnerChanged,
}

/// A pipe that failed, as the run's refusal (74).
fn no_pipe(why: &str) -> Refusal {
    (WatchExit::NoPipe, format!("the pipe: {why}"))
}

/// Serves `pool`'s refusals until the lifetime ends or the record's owner changes: `Ok` with
/// the log line's counts, or the trace ending or the pipe failing.
pub fn serve(w: &mut impl Watcher, pool: &Pool) -> Result<String, Refusal> {
    let users = pool.users.as_slice();
    let mut life = Lifetime::new(w.now());
    let mut rate = RateLimit::new(w.now());
    let mut checked_at = w.now();
    let rate_log = w.rate_log();
    let mut breaker = Breaker::new(w.now(), w.events());
    let (mut sent, mut dropped, mut refused) = (0u64, 0u64, 0u64);
    let stop = loop {
        let records = match w.records(TICK) {
            Delivery::Records(records, lost) => {
                rate.lose(lost);
                records
            }
            Delivery::Ended => {
                return Err((
                    WatchExit::TraceLost,
                    format!("the trace ended while serving ({sent} sent, {dropped} dropped)"),
                ));
            }
        };
        let now = w.now();
        for record in &records {
            // The consumer passes on only a pool user's refusal; checked again where it is
            // written, so a bug there can't send anyone else's.
            let Some(line) = denied_line(record, users) else {
                refused += 1;
                continue;
            };
            if rate.admit(now) {
                w.send(&line).map_err(|why| no_pipe(&why))?;
                sent += 1;
            }
        }
        if let Some(n) = rate.roll(now) {
            dropped += n;
            w.send(&dropped_line(n)).map_err(|why| no_pipe(&why))?;
        }
        let clients = w
            .clients()
            .map_err(|why| no_pipe(&format!("its clients couldn't be polled: {why}")))?;
        brake(w, &mut breaker, now, rate_log)?;
        if now.saturating_sub(checked_at) >= OWNER_CHECK {
            checked_at = now;
            // A record that can't be read now is read again at the next check.
            let changed = match w.pool() {
                Ok(Some(again)) => again.owner != pool.owner,
                Ok(None) => true,
                Err(_unread) => false,
            };
            if changed {
                break Stop::OwnerChanged;
            }
        }
        match life.step(now, clients) {
            Verdict::Go => {}
            verdict @ (Verdict::NoClient | Verdict::LastLeft | Verdict::TooLong) => {
                break Stop::Life(verdict);
            }
        }
    };
    let why = match stop {
        Stop::Life(Verdict::NoClient) => "no client connected",
        Stop::Life(Verdict::LastLeft) => "the last client left",
        Stop::Life(Verdict::TooLong | Verdict::Go) => "the run reached its limit",
        Stop::OwnerChanged => "the record's owner changed",
    };
    let refused = if refused == 0 {
        String::new()
    } else {
        format!(", {refused} not of the pool refused")
    };
    let trips = match breaker.trips() {
        0 => String::new(),
        n => format!(", paused {n} times for the event rate"),
    };
    Ok(format!(
        "{why}; {} clients, {sent} sent, {dropped} dropped{refused}{trips}",
        life.served()
    ))
}

/// Steps the breaker at `now`: logs the second's rate when asked, and pauses or resumes the
/// trace, telling the clients. A trace that can't be paused or resumed ends the run (73).
fn brake(
    w: &mut impl Watcher,
    breaker: &mut Breaker,
    now: Duration,
    rate_log: bool,
) -> Result<(), Refusal> {
    let step = breaker.step(now, w.events());
    if rate_log && let Some(rate) = step.second {
        w.log(
            false,
            &format!("the sandbox's watcher's event rate: {rate}"),
        );
    }
    let no_trace = |why: String| (WatchExit::NoTrace, format!("the breaker: {why}"));
    match step.action {
        Some(Action::Pause) => {
            w.pause_trace().map_err(no_trace)?;
            w.send(&paused_line(PAUSE.as_secs()))
                .map_err(|why| no_pipe(&why))?;
            let n = breaker.trips();
            w.log(
                false,
                &format!("the sandbox's watcher: paused for the event rate ({n})"),
            );
        }
        Some(Action::Resume) => {
            w.resume_trace().map_err(no_trace)?;
            w.send(&resumed_line()).map_err(|why| no_pipe(&why))?;
            w.log(false, "the sandbox's watcher: resumed after its pause");
        }
        None => {}
    }
    Ok(())
}
