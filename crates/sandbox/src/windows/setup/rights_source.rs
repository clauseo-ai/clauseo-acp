//! [`PolicySource`] on Windows: the Group Policy template cache's files, the Intune
//! `PolicyManager` key, and name lookups (`rights_policy.rs` says what is read and why). Plus
//! [`read_now`] for set-up and [`cached`] for a launch, which keeps the parsed lists in one
//! process-wide [`PolicyCache`]. The cache's folder and key are readable by administrators and
//! SYSTEM only, so a standard user's read is [`RightPolicy::Unknown`].

use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use super::ffi::{reg_last_write, reg_string, windows_dir};
use super::preflight::sid_of_name;
use super::rights_policy::{
    CACHE_TTL, MDM_KEY, PolicyCache, PolicySource, Pool, RightPolicy, read_policy,
};
use crate::windows::sids::Sid;

/// The real source.
#[derive(Debug, Default)]
pub struct WindowsPolicySource;

impl PolicySource for WindowsPolicySource {
    fn cache_files(&mut self) -> io::Result<Vec<(PathBuf, u64)>> {
        let dir = windows_dir()
            .map_err(io::Error::other)?
            .join("security")
            .join("templates")
            .join("policies");
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let meta = entry.metadata()?;
            if !meta.is_file() {
                continue;
            }
            let modified = meta
                .modified()?
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default();
            let stamp = u64::try_from(modified.as_nanos()).unwrap_or(u64::MAX);
            files.push((entry.path(), stamp));
        }
        Ok(files)
    }

    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn mdm_stamp(&mut self) -> io::Result<Option<u64>> {
        reg_last_write(MDM_KEY).map_err(io::Error::other)
    }

    fn mdm_value(&mut self, name: &str) -> io::Result<Option<String>> {
        reg_string(MDM_KEY, name).map_err(io::Error::other)
    }

    fn resolve(&mut self, name: &str) -> Option<Sid> {
        match sid_of_name(name) {
            Ok(found) => found.map(|(sid, _)| sid),
            Err(error) => {
                tracing::debug!(%error, "a name in a logon-right policy didn't resolve");
                None
            }
        }
    }
}

/// What policy says of `right` for `pool`, read afresh: set-up's check, elevated.
pub fn read_now(right: &str, pool: Pool<'_>) -> RightPolicy {
    read_policy(&mut WindowsPolicySource, right, pool)
}

/// What policy says of `right` for `pool`, through the process-wide cache (`rights_policy.rs`):
/// for a SYSTEM-side launch, which may call it every time. `now` is monotonic time since any
/// fixed start (the caller's clock, as the core takes time). Never reads WMI.
pub fn cached(right: &str, pool: Pool<'_>, now: Duration) -> RightPolicy {
    static CACHE: Mutex<Option<PolicyCache>> = Mutex::new(None);
    let mut held = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
    held.get_or_insert_with(|| PolicyCache::new(CACHE_TTL))
        .right(&mut WindowsPolicySource, right, pool, now)
}
