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

/// The files [`bring_back_after_delete`] armed, by absolute path.
static BRING_BACK: std::sync::Mutex<Vec<std::path::PathBuf>> = std::sync::Mutex::new(Vec::new());

/// From now on, for the whole process, the next delete of the file `path` (absolute) by this
/// crate is undone at once, with the bytes the file had: a sync client putting back a file an
/// undo deleted before the undo's checkpoint walks (`Undone::resurrected`). Once per call.
/// Process-wide, unlike the unit tests' per-thread hooks, because the host deletes on a matter
/// thread; it suits nextest's one process per test.
pub fn bring_back_after_delete(path: impl Into<std::path::PathBuf>) {
    let mut armed = BRING_BACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    armed.push(path.into());
}

/// Before the delete of `path`: its bytes when [`bring_back_after_delete`] armed it (and
/// disarms it).
pub(crate) fn before_delete(path: &std::path::Path) -> Option<Vec<u8>> {
    let mut armed = BRING_BACK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = armed.iter().position(|one| one == path)?;
    armed.remove(at);
    drop(armed);
    std::fs::read(path).ok()
}

/// After `path` was deleted: writes `bytes` there again. A failure is the test's to see.
pub(crate) fn after_delete(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write as _;
    let written = crate::write::create_new(path).and_then(|mut file| file.write_all(bytes));
    if let Err(err) = written {
        tracing::warn!(error = %err, "a test's file was not brought back");
    }
}
