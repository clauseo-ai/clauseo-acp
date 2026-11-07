//! When the next round runs: the interval jittered by up to a twelfth of it either way (± 30
//! min at 6 h), so a fleet of hosts does not fetch at once, and one short retry after a
//! signature mismatch, since the release's two files are replaced one after the other and a
//! round in that window sees the new list beside the old signature.

use std::time::Duration;

use super::Round;

/// The wait before the one retry after a signature mismatch: about the publish window.
pub(crate) const MISMATCH_RETRY: Duration = Duration::from_mins(10);

/// The wait after `round`, and whether that wait is the mismatch retry. `retried` says the
/// round just run was the retry, so a second mismatch waits the whole interval. `random`
/// places the jitter.
pub(crate) fn next_wait(
    interval: Duration,
    round: Round,
    retried: bool,
    random: u64,
) -> (Duration, bool) {
    if round == Round::Mismatch && !retried {
        return (MISMATCH_RETRY, true);
    }
    let spread = interval / 12;
    let span = u64::try_from((spread * 2).as_millis()).unwrap_or(u64::MAX);
    let offset = Duration::from_millis(random % span.saturating_add(1));
    (
        interval.saturating_sub(spread).saturating_add(offset),
        false,
    )
}

/// A random `u64` from the OS; 0 when it has none (the shortest wait).
pub(crate) fn random() -> u64 {
    let mut bytes = [0_u8; 8];
    getrandom::fill(&mut bytes).map_or(0, |()| u64::from_le_bytes(bytes))
}
