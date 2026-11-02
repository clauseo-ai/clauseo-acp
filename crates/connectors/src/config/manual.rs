//! [`ManualClock`]: a clock tests move by hand.

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

use super::Clock;

/// A clock that stands still until a test moves it. Share it as `Arc<ManualClock>` between the
/// [`Config`](crate::Config) and the fake connector's options.
#[derive(Debug)]
pub struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    /// 2026-11-02 00:00:00 UTC, a fixed start for tests.
    pub const START: Duration = Duration::from_hours(498_216);

    /// A clock at `now`.
    pub fn new(now: SystemTime) -> ManualClock {
        ManualClock {
            now: Mutex::new(now),
        }
    }

    /// Moves the clock forward by `by`.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock().unwrap_or_else(PoisonError::into_inner);
        *now = now.checked_add(by).unwrap_or(*now);
    }

    /// Sets the clock to `to`.
    pub fn set(&self, to: SystemTime) {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner) = to;
    }
}

impl Default for ManualClock {
    /// A clock at [`ManualClock::START`] after the Unix epoch.
    fn default() -> ManualClock {
        ManualClock::new(SystemTime::UNIX_EPOCH + ManualClock::START)
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
