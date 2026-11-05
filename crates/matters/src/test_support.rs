//! Test helpers, behind the `test-support` feature, which only dev-dependencies turn on (this
//! crate's own and `host`'s); the app's build never has it.

use std::sync::atomic::{AtomicBool, Ordering};

/// Set by [`pretend_word_running_since_epoch`]; never cleared.
static PRETEND_WORD: AtomicBool = AtomicBool::new(false);

/// From now on, for the whole process, the stale owner-file rule hears that one Word process
/// runs, started at the Unix epoch, in place of the real process listing. The rule still runs
/// and answers held for an owner file a test makes, since a Word runs (on macOS and Windows
/// alike). (Linux has no stale rule: an owner file is always
/// held.) Tests that plant an owner file with no Word running call it from their fixture;
/// the tests of the stale rule itself live in test binaries that never do. Process-wide, so it
/// suits nextest's one process per test.
pub fn pretend_word_running_since_epoch() {
    PRETEND_WORD.store(true, Ordering::Relaxed);
}

/// Whether [`pretend_word_running_since_epoch`] was called.
#[cfg(any(target_os = "macos", windows))]
pub(crate) fn pretending() -> bool {
    PRETEND_WORD.load(Ordering::Relaxed)
}
