//! The only clock read in xtask. The check steps and `buildtime` both time their work through it.

use std::time::Instant;

/// Runs `work` and returns its value with the seconds it took.
#[expect(clippy::disallowed_methods, reason = "xtask runs tools and times them")]
pub(crate) fn timed<T>(work: impl FnOnce() -> T) -> (T, f64) {
    let started = Instant::now();
    let value = work();
    (value, started.elapsed().as_secs_f64())
}
