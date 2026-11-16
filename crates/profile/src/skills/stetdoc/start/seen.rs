//! The marker of the stetdoc last seen to answer (`ProfilePaths::stetdoc_seen`): its path, size
//! and modification time, one per line. A first run after an install or an update (a new file,
//! or the same path with another size or time) is not seen yet, so [`super::start`] does not
//! wait for it while the OS may scan it and leaves it to the background tries; one seen before
//! gets one try within [`super::Limits::seen`] at the start.

use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::write;

/// What the marker holds for `program` as it is on disk now; `None` when its file does not
/// read.
fn key(program: &Path) -> Option<String> {
    let meta = std::fs::metadata(program).ok()?;
    let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(format!(
        "{}\n{}\n{}.{:09}\n",
        program.to_string_lossy(),
        meta.len(),
        modified.as_secs(),
        modified.subsec_nanos()
    ))
}

/// Whether `marker` names `program` as it is on disk now. A missing or unreadable marker, or
/// a program whose file does not read, is not seen.
#[must_use]
pub fn is_seen(marker: &Path, program: &Path) -> bool {
    let Some(key) = key(program) else {
        return false;
    };
    std::fs::read_to_string(marker).is_ok_and(|held| held == key)
}

/// Records `program` in `marker` as seen to answer. A marker that cannot be written is logged:
/// the next start then gives stetdoc the longer limit again, and nothing else changes.
pub fn record(marker: &Path, program: &Path) {
    let Some(key) = key(program) else {
        return;
    };
    if let Err(err) = write::replace_if_differs(marker, key.as_bytes()) {
        tracing::warn!(%err, "stetdoc answered, but the marker that says so was not written");
    }
}
