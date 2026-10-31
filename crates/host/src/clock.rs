//! The host's one clock: wall time for events, monotonic time for timers, and timers themselves.
//! [`SystemClock`] reads the OS; `ManualClock` (feature `test-support`) is moved by tests.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use clauseo_protocol::Timestamp;

#[cfg(feature = "test-support")]
mod manual;

#[cfg(feature = "test-support")]
pub use manual::ManualClock;

/// One reading of the clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Now {
    /// Wall time, for event timestamps (display only).
    pub wall: Timestamp,
    /// Monotonic time since a fixed base, for deadlines and timers.
    pub mono: Duration,
}

/// A source of time and timers. The host reads time only through this.
pub trait Clock: Send + Sync {
    /// The current wall and monotonic time.
    fn now(&self) -> Now;
    /// A future that completes `d` after this call.
    fn after(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// The OS clock, with timers on async-io's reactor.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

/// The monotonic base, set by the first reading.
static MONO_BASE: OnceLock<Instant> = OnceLock::new();

impl Clock for SystemClock {
    #[expect(clippy::disallowed_methods, reason = "the host's one clock read")]
    fn now(&self) -> Now {
        let instant = Instant::now();
        let base = *MONO_BASE.get_or_init(|| instant);
        let millis = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| {
                i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
            });
        Now {
            wall: Timestamp::new(millis).unwrap_or(Timestamp::ZERO),
            mono: instant.saturating_duration_since(base),
        }
    }

    fn after(&self, d: Duration) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(async move {
            async_io::Timer::after(d).await;
        })
    }
}

/// The host clock as the process crate's timer, for the shutdown ladder.
pub(crate) struct ProcessTimer(pub(crate) Arc<dyn Clock>);

impl clauseo_process::Timer for ProcessTimer {
    fn after(&self, duration: Duration) -> impl Future<Output = ()> + Send {
        self.0.after(duration)
    }
}

#[cfg(test)]
mod tests;
