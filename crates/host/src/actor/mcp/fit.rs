//! How many of an import's servers its one confirm shows: whole servers, in order, while the
//! confirm's text, measured as shown (`config::mcp::shown`), stays within [`MAX_ASK_CHARS`]. A
//! server is never shown in part; the ones after the first that does not fit are left out whole.

use crate::config::mcp::McpLaunchAsk;
use crate::config::mcp::shown::{self, MAX_ASK_CHARS};

/// How many of `asks`, from the first, fit in one confirm. Stops at the first that does not fit,
/// so the confirm's text is measured at most once per server kept, plus one.
pub(super) fn fitting(asks: &[McpLaunchAsk]) -> usize {
    (1..=asks.len())
        .take_while(|&n| {
            asks.get(..n)
                .is_some_and(|some| shown::import_chars(some) <= MAX_ASK_CHARS)
        })
        .last()
        .unwrap_or(0)
}

/// Whether `ask` fits in a confirm on its own.
pub(super) fn fits_alone(ask: &McpLaunchAsk) -> bool {
    shown::import_chars(std::slice::from_ref(ask)) <= MAX_ASK_CHARS
}
