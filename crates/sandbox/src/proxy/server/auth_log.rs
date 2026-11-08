//! The cap on the egress log's 407 lines: 5, then one per 10 s. Any program that can reach the
//! agent listener without its token (another of the lawyer's programs, say; on Windows no
//! other run's agent, which set-up's filters keep off every slot's ports) can send requests
//! naming hosts of its choosing. One bucket for the whole proxy, not one per connection or
//! source port (each connection gets a new one), keeps them from filling this run's log. The
//! next 407 record written carries the count dropped before it (`Exchange::suppressed`), so
//! the log says "N more 407s suppressed"; a flood's last drops, with no 407 after them, go
//! uncounted in the log.

use std::time::Duration;

/// The 407 lines the bucket holds when full.
const CAPACITY: u32 = 5;
/// The time that earns one more line.
const REFILL: Duration = Duration::from_secs(10);

/// What to do with one 407's log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Admit {
    /// Write it, after a line naming the 407 lines dropped since the last one, if any.
    Log { suppressed: u64 },
    /// Drop it; it is counted for the next line written.
    Drop,
}

/// A token bucket of 407 log lines, read with the time from the proxy's clock.
#[derive(Debug)]
pub(crate) struct AuthLog {
    tokens: u32,
    /// When the bucket last earned a line, or was last full.
    refilled: Duration,
    suppressed: u64,
}

impl AuthLog {
    /// A full bucket at `now`.
    pub(crate) const fn new(now: Duration) -> Self {
        Self {
            tokens: CAPACITY,
            refilled: now,
            suppressed: 0,
        }
    }

    /// Spends one line if the bucket has one at `now`. A clock that went back earns nothing.
    pub(crate) fn admit(&mut self, now: Duration) -> Admit {
        let earned = now.saturating_sub(self.refilled).as_nanos() / REFILL.as_nanos();
        let earned = u32::try_from(earned).unwrap_or(u32::MAX);
        if earned > 0 {
            self.tokens = self.tokens.saturating_add(earned).min(CAPACITY);
            self.refilled = if self.tokens == CAPACITY {
                now
            } else {
                // Short of full, so `earned` is below `CAPACITY` and the product cannot wrap.
                self.refilled + REFILL * earned
            };
        }
        if self.tokens == 0 {
            self.suppressed = self.suppressed.saturating_add(1);
            return Admit::Drop;
        }
        if self.tokens == CAPACITY {
            // A full bucket earned nothing while full: the next line is earned from now.
            self.refilled = now.max(self.refilled);
        }
        self.tokens -= 1;
        Admit::Log {
            suppressed: std::mem::take(&mut self.suppressed),
        }
    }
}
