//! The chooser's timing lines in host.log: one info line per step from the click to the first
//! listing, each with the milliseconds since the app started (`at_ms`, monotonic) and since the
//! step's own start (`took_ms`), and never a path, so a slow open can be broken down from the log
//! alone. The page's own marks come through `chooser_shown`.

use std::sync::OnceLock;
use std::time::Instant;

/// The target of these lines: host.log keeps info only from `clauseo_*::lifecycle` targets
/// (`clauseo_host::log_file`), so a plain module target would never reach the file.
pub(crate) const TARGET: &str = "clauseo_desktop::chooser::lifecycle";

/// When the app started, as `lib::run` marks it first thing.
static LAUNCH: OnceLock<Instant> = OnceLock::new();

/// Marks the app's start; later calls change nothing.
pub(crate) fn launch() {
    LAUNCH.get_or_init(crate::open::now);
}

/// The milliseconds from `at` to now.
pub(crate) fn since(at: Instant) -> u64 {
    let gone = crate::open::now().saturating_duration_since(at);
    u64::try_from(gone.as_millis()).unwrap_or(u64::MAX)
}

/// The milliseconds since the app started.
pub(crate) fn at_ms() -> u64 {
    since(*LAUNCH.get_or_init(crate::open::now))
}

/// Logs the chooser's step `what`, begun at `began`, with `detail` (no path).
pub(crate) fn step(what: &str, began: Instant, detail: &str) {
    tracing::info!(
        target: TARGET,
        at_ms = at_ms(),
        took_ms = since(began),
        detail,
        "folder chooser: {what}"
    );
}

/// Logs the start-up's step `what`, begun at `began`, on the same clock: the click's lines
/// read against when the host came up.
pub(crate) fn mark(what: &str, began: Instant, detail: &str) {
    tracing::info!(target: TARGET, at_ms = at_ms(), took_ms = since(began), detail, "{what}");
}

#[cfg(test)]
mod tests {
    /// host.log keeps info lines only from a `clauseo_*` crate's `::lifecycle` target.
    #[test]
    fn the_target_is_one_host_log_keeps() {
        assert!(super::TARGET.starts_with("clauseo_"));
        assert!(super::TARGET.ends_with("::lifecycle"));
    }
}
