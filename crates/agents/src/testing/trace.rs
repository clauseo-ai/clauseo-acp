//! Timestamped phase lines on stderr, for tests only: nextest shows a test's stderr when it
//! times out, so a stall's output says which phase the time went to (issue: Windows
//! install-test stalls, S18). Each line carries the seconds since this process's first line and
//! the UTC time of day, which lines up with the CI log's own timestamps.

use std::fmt;
use std::sync::OnceLock;
use std::time::{Duration, Instant, SystemTime};

/// The first line's instant.
static START: OnceLock<Instant> = OnceLock::new();

/// Writes `[agents-test +S.mmms HH:MM:SS.mmmZ] <line>` to stderr.
#[expect(
    clippy::disallowed_methods,
    reason = "test diagnostics: a stalled test's log needs the real time, not an injected clock"
)]
#[expect(
    clippy::print_stderr,
    reason = "test diagnostics: nextest shows a timed-out test's stderr"
)]
pub fn trace(line: fmt::Arguments<'_>) {
    let now = Instant::now();
    let since = now.saturating_duration_since(*START.get_or_init(|| now));
    let wall = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::ZERO);
    let day = wall.as_secs() % 86_400;
    let (hours, minutes, seconds) = (day / 3600, day / 60 % 60, day % 60);
    let millis = wall.subsec_millis();
    eprintln!(
        "[agents-test +{:.3}s {hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z] {line}",
        since.as_secs_f64()
    );
}

/// Milliseconds since `start`, for a phase's own duration.
pub fn millis_since(start: Instant) -> u128 {
    start.elapsed().as_millis()
}

/// Now, for [`millis_since`].
#[expect(
    clippy::disallowed_methods,
    reason = "test diagnostics: a stalled test's log needs the real time, not an injected clock"
)]
pub fn instant() -> Instant {
    Instant::now()
}
