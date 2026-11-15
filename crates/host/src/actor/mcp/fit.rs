//! How many of an import's servers its one confirm shows: whole servers, in order, while the
//! confirm's text, measured as shown (`config::mcp::shown`), stays within [`MAX_ASK_CHARS`]. A
//! server is never shown in part; the ones after the first that does not fit are left out whole.

use crate::config::mcp::McpLaunchAsk;
use crate::config::mcp::shown::{self, MAX_ASK_CHARS};

/// How many of `asks`, from the first, fit in one confirm. Stops at the first that does not fit,
/// so the confirm's text is measured at most once per server kept, plus one.
pub(super) fn fitting(asks: &[McpLaunchAsk]) -> usize {
    let mut kept = 0;
    while kept < asks.len() && shown::import_chars(&asks[..=kept]) <= MAX_ASK_CHARS {
        kept += 1;
    }
    kept
}

/// Whether `ask` fits in a confirm on its own.
pub(super) fn fits_alone(ask: &McpLaunchAsk) -> bool {
    shown::import_chars(std::slice::from_ref(ask)) <= MAX_ASK_CHARS
}
