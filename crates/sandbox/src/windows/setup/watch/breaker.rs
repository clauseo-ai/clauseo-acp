//! The event-rate breaker (the orchestrator's ruling on the cost), with no Windows call: nearly
//! all the reporter's cost is the kernel generating Kernel-File events, and that cost grows with
//! the machine's file activity, not the pool's. So when the machine is that busy for long, the
//! watcher stops asking for the events for a while.
//!
//! - The callback counts every Kernel-File event, before the SID check (one relaxed atomic add).
//! - Once a second the serving loop takes the count since the last second, as a rate.
//! - At or above [`PAUSE_RATE`] for [`WINDOW_SECONDS`] seconds in a row, it trips: the provider
//!   is disabled on the session, the pending Creates are cleared, and the clients are told
//!   `{"v":1,"paused":{"secs":60,"reason":"rate"}}`. After [`PAUSE`] the provider is enabled
//!   again and the clients are told `{"v":1,"resumed":true}`. Each trip is logged, as counts.
//!
//! The spike measured 235 events/s idle, about 3,600/s for an editing turn and about 10,850/s
//! on average during `npm ci`. 8,000/s trips on a long `npm ci` and never on editing; VM row W12
//! sets the final value from the per-second distribution `WatchRateLog` logs.

use std::time::Duration;

/// The rate, in Kernel-File events a second, at or above which a second counts as hot.
pub const PAUSE_RATE: u64 = 8000;
/// How many hot seconds in a row trip the breaker.
pub const WINDOW_SECONDS: u32 = 10;
/// How long the provider stays disabled once tripped.
pub const PAUSE: Duration = Duration::from_secs(60);
/// One bucket.
const SECOND: Duration = Duration::from_secs(1);

/// What one step asks of the serving loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Disable the provider, clear the pending Creates and tell the clients.
    Pause,
    /// Enable the provider again and tell the clients.
    Resume,
}

/// What one step found.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Step {
    /// The rate of the second that just ended, when one did (for `WatchRateLog`).
    pub second: Option<u64>,
    /// What to do now.
    pub action: Option<Action>,
}

/// The breaker's state.
#[derive(Debug, Clone)]
pub struct Breaker {
    /// When the current bucket began, and the event count then.
    began: Duration,
    count_then: u64,
    /// Hot seconds in a row.
    hot: u32,
    /// While paused, when it resumes.
    until: Option<Duration>,
    /// How many times it tripped, for the log.
    trips: u64,
}

impl Breaker {
    /// A breaker whose first bucket begins at `now`, with the callback's count at `count`.
    pub fn new(now: Duration, count: u64) -> Breaker {
        Breaker {
            began: now,
            count_then: count,
            hot: 0,
            until: None,
            trips: 0,
        }
    }

    /// How many times it tripped.
    pub fn trips(&self) -> u64 {
        self.trips
    }

    /// One tick at `now`, the callback's count of every event so far at `count`.
    pub fn step(&mut self, now: Duration, count: u64) -> Step {
        let mut step = Step::default();
        let elapsed = now.saturating_sub(self.began);
        if elapsed >= SECOND {
            let events = count.saturating_sub(self.count_then);
            let millis = u64::try_from(elapsed.as_millis())
                .unwrap_or(u64::MAX)
                .max(1);
            let rate = events.saturating_mul(1000) / millis;
            step.second = Some(rate);
            self.began = now;
            self.count_then = count;
            self.hot = if self.until.is_none() && rate >= PAUSE_RATE {
                self.hot + 1
            } else {
                0
            };
        }
        if let Some(until) = self.until {
            if now >= until {
                self.until = None;
                self.hot = 0;
                step.action = Some(Action::Resume);
            }
        } else if self.hot >= WINDOW_SECONDS {
            self.hot = 0;
            self.until = Some(now + PAUSE);
            self.trips += 1;
            step.action = Some(Action::Pause);
        }
        step
    }
}
