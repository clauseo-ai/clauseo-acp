//! Step 6, the serving loop, with no Windows call: once per [`TICK`] it takes what the consumer
//! produced, sends each refusal the rate admits, sends what was dropped once a second, polls
//! the clients and steps the [`Lifetime`].

use std::time::Duration;

use super::lifetime::{Lifetime, Verdict};
use super::protocol::{RateLimit, denied_line, dropped_line};
use super::{Delivery, Refusal, WatchExit, Watcher};
use crate::windows::sids::Sid;

/// How long one wait for the consumer lasts: the loop's tick.
pub(super) const TICK: Duration = Duration::from_millis(250);

/// Serves `pool`'s refusals until the lifetime ends: `Ok` with the log line's counts, or the
/// trace ending or the clients failing to poll.
pub fn serve(w: &mut impl Watcher, pool: &[Sid]) -> Result<String, Refusal> {
    let mut life = Lifetime::new(w.now());
    let mut rate = RateLimit::new(w.now());
    let (mut sent, mut dropped, mut refused) = (0u64, 0u64, 0u64);
    let verdict = loop {
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
            let Some(line) = denied_line(record, pool) else {
                refused += 1;
                continue;
            };
            if rate.admit(now) {
                w.send(&line);
                sent += 1;
            }
        }
        if let Some(n) = rate.roll(now) {
            dropped += n;
            w.send(&dropped_line(n));
        }
        let clients = w.clients().map_err(|why| {
            (
                WatchExit::NoPipe,
                format!("the pipe's clients couldn't be polled: {why}"),
            )
        })?;
        match life.step(now, clients) {
            Verdict::Go => {}
            verdict @ (Verdict::NoClient | Verdict::LastLeft | Verdict::TooLong) => {
                break verdict;
            }
        }
    };
    let why = match verdict {
        Verdict::NoClient => "no client connected",
        Verdict::LastLeft => "the last client left",
        Verdict::TooLong | Verdict::Go => "the run reached its limit",
    };
    let refused = if refused == 0 {
        String::new()
    } else {
        format!(", {refused} not of the pool refused")
    };
    Ok(format!(
        "{why}; {} clients, {sent} sent, {dropped} dropped{refused}",
        life.served()
    ))
}
