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

/// The files [`fail_after_replace_write`] armed, by absolute path.
static FAIL_AFTER_WRITE: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// From now on, for the whole process, the next `Matter::replace_file` that writes the file
/// `path` (absolute) fails right after its rename, before its `revisions` checkpoint, with a
/// sharing violation: as that checkpoint's walk fails on Windows when it meets a file another
/// program holds. Once per call. Process-wide, as the host replaces on a matter thread; it suits
/// nextest's one process per test.
pub fn fail_after_replace_write(path: impl Into<std::path::PathBuf>) {
    let mut armed = FAIL_AFTER_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    armed.push(path.into());
}

/// After a replace renamed its file over `path`: the armed failure, once.
pub(crate) fn after_replace_write(path: &std::path::Path) -> Result<(), crate::Error> {
    let mut armed = FAIL_AFTER_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(at) = armed.iter().position(|one| one == path) else {
        return Ok(());
    };
    armed.remove(at);
    Err(crate::Error::io(
        "read",
        path,
        std::io::Error::from_raw_os_error(32),
    ))
}

/// The holds [`hold_at_replace_write`] armed: the file's absolute path, and the bytes written
/// to it while held, if any.
type Hold = (std::path::PathBuf, Option<Vec<u8>>);

/// The holds [`hold_at_replace_write`] armed.
static HOLD_AT_WRITE: std::sync::Mutex<Vec<Hold>> = std::sync::Mutex::new(Vec::new());

/// From now on, for the whole process, the next write of `Matter::replace_file` to the file
/// `path` (absolute) fails before it renames, as if a program held the file (an antivirus
/// scan, on Windows) and let go before the replace checks again. With `changed`, those bytes
/// are written to the file first: another program's real change while it held the file. Once
/// per call. Process-wide, as the host replaces on a matter thread; it suits nextest's one
/// process per test.
pub fn hold_at_replace_write(path: impl Into<std::path::PathBuf>, changed: Option<Vec<u8>>) {
    let mut armed = HOLD_AT_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    armed.push((path.into(), changed));
}

/// Before a replace's write to `path`: whether a hold was armed for it (and disarms it), after
/// writing its bytes, if any. A failed write is the test's to see.
pub(crate) fn held_at_replace_write(path: &std::path::Path) -> bool {
    let mut armed = HOLD_AT_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(at) = armed.iter().position(|(one, _)| one == path) else {
        return false;
    };
    let (_, changed) = armed.remove(at);
    drop(armed);
    if let Some(bytes) = changed {
        use std::io::Write as _;
        let written = crate::write::remove_file(path)
            .and_then(|()| crate::write::create_new(path))
            .and_then(|mut file| file.write_all(&bytes));
        if let Err(err) = written {
            tracing::warn!(error = %err, "a test's change of a held file was not written");
        }
    }
    true
}

/// The files [`fail_restore_write`] armed, by absolute path.
static FAIL_RESTORE_WRITE: std::sync::Mutex<Vec<std::path::PathBuf>> =
    std::sync::Mutex::new(Vec::new());

/// From now on, for the whole process, the next `Matter::restore` that would write or delete
/// the file `path` (absolute) fails after its outside-edits checkpoint, before it writes
/// anything, with a sharing violation: where a crash during its write-back would stop it. Once
/// per call. Process-wide, as the host restores on a matter thread; it suits nextest's one
/// process per test.
pub fn fail_restore_write(path: impl Into<std::path::PathBuf>) {
    let mut armed = FAIL_RESTORE_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    armed.push(path.into());
}

/// Before a restore in `folder` writes `targets`: the armed failure of the first one armed,
/// once.
pub(crate) fn before_restore_write(
    folder: &std::path::Path,
    targets: &[clauseo_protocol::MatterPath],
) -> Result<(), crate::Error> {
    let mut armed = FAIL_RESTORE_WRITE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for target in targets {
        let path = folder.join(target.as_str());
        if let Some(at) = armed.iter().position(|one| *one == path) {
            armed.remove(at);
            return Err(crate::Error::io(
                "write",
                path,
                std::io::Error::from_raw_os_error(32),
            ));
        }
    }
    Ok(())
}
