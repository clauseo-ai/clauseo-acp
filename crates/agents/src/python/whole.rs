//! The whole-check: every file the runtime's manifest lists is a regular file (read through no
//! link) of its listed size. One stat per file and no file read. On Windows the standard
//! library's stat opens a handle with no access rights (attributes only), and Windows
//! Defender's on-access scan runs when a file is opened for its data or run, not then.
//!
//! A released runtime holds about 5,800 files, about 17 ms of stats on a warm Linux disk and
//! more on Windows, and the check is asked for at every session setup. So its result is kept
//! for the rest of this start, per runtime folder and manifest; an install or a copy moved
//! back into place forgets it ([`forget`]). A file that goes in the middle of a session is
//! noticed at the next start, as an agent's or Node's is.
//!
//! The limit of a size check: a file changed to other bytes of the same size still passes.
//! Only the repair check ([`super::contents_match`]) reads and hashes every byte.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

/// Each runtime folder's last whole-check this start: the manifest's SHA-256 it ran against,
/// and its result.
static WHOLE: OnceLock<Mutex<HashMap<PathBuf, (String, bool)>>> = OnceLock::new();

fn checks() -> MutexGuard<'static, HashMap<PathBuf, (String, bool)>> {
    WHOLE
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Whether every file `manifest` (whose SHA-256 is `manifest_sha256`) lists is in `dir` at its
/// size: the result kept from earlier this start for the same folder and manifest, else
/// [`sizes_match`]'s, then kept.
pub(super) fn whole(dir: &Path, manifest_sha256: &str, manifest: &str) -> bool {
    if let Some((sha, whole)) = checks().get(dir)
        && sha == manifest_sha256
    {
        return *whole;
    }
    let whole = sizes_match(dir, manifest);
    checks().insert(dir.to_path_buf(), (manifest_sha256.to_owned(), whole));
    whole
}

/// Forgets `dir`'s kept result: its files are about to change, or have.
pub(super) fn forget(dir: &Path) {
    checks().remove(dir);
}

/// Whether each `<sha256> <size> <path>` line of `manifest` names a regular file in `dir`
/// (read through no link) of that size. A line that does not parse fails.
pub(super) fn sizes_match(dir: &Path, manifest: &str) -> bool {
    manifest.lines().all(|line| {
        let mut parts = line.splitn(3, ' ');
        let (Some(_), Some(size), Some(rel)) = (parts.next(), parts.next(), parts.next()) else {
            return false;
        };
        let Ok(size) = size.parse::<u64>() else {
            return false;
        };
        let path = super::manifest_path(dir, rel);
        std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() == size)
    })
}
