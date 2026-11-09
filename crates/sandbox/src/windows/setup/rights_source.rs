//! [`PolicySource`] on Windows: the Group Policy template cache's files, the Intune
//! `PolicyManager` key, and name lookups (`rights_policy.rs` says what is read and why). Plus
//! [`read_now`] for set-up and [`cached`] for a launch, which keeps the parsed lists in one
//! process-wide [`PolicyCache`]. The cache's folder and key are readable by administrators and
//! SYSTEM only, so a standard user's read is [`RightPolicy::Unknown`].

use std::fs::Metadata;
use std::io;
use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use windows::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

use super::ffi::{reg_last_write, reg_string, windows_dir};
use super::preflight::sid_of_name;
use super::record_write;
use super::result_file::now_secs;
use super::rights_policy::{
    CACHE_TTL, INTERACTIVE_RIGHT, MDM_KEY, PolicyCache, PolicySource, PolicyState, Pool,
    RightPolicy, read_policy,
};
use crate::windows::refresh::source::read_capped;
use crate::windows::sids::Sid;

/// The most a template is read to: GPO templates are a few kilobytes.
const FILE_CAP: u64 = 4 << 20;

/// Whether `meta`, read without following, is a reparse point (a link or junction).
fn is_link(meta: &Metadata) -> bool {
    meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
}

fn not_plain() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "a policy template, or its folder, is a link or not a plain file",
    )
}

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
        // The folder itself, not what a link there names: a link is refused (`Unknown`).
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if is_link(&meta) || !meta.is_dir() => return Err(not_plain()),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            // A directory entry's metadata is the name's own on Windows, never a link's target.
            let meta = entry.metadata()?;
            if is_link(&meta) || !meta.is_file() {
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

    /// Opened as itself (`refresh::source::read_capped`: `FILE_FLAG_OPEN_REPARSE_POINT`), so a
    /// link swapped in after the listing is refused by its tag, never followed; a regular file
    /// with one link, read up to [`FILE_CAP`] bytes.
    fn read_file(&mut self, path: &Path) -> io::Result<Vec<u8>> {
        read_capped(path, FILE_CAP)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
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

/// The SYSTEM refresh's update of the record's `LogonPolicy` (`exe.rs`, before the refresh):
/// what policy says now of "Allow log on locally" for the record's group and pool, so a policy
/// that changed since set-up names itself at a launch's 1385. Only into a record that is there
/// and not being removed (never made again); every failure is logged and changes nothing else.
pub fn record_for_launches(record_key: &str) {
    let record = match record_write::read(record_key) {
        Ok(Some(record)) if !record.removing => record,
        Ok(_) => return,
        Err(error) => {
            tracing::warn!(%error, "the record couldn't be read for LogonPolicy");
            return;
        }
    };
    let pool = Pool {
        group: Some(&record.group),
        users: &record.pool,
    };
    let state = PolicyState::of(read_now(INTERACTIVE_RIGHT, pool));
    if let Err(error) = record_write::write_logon_policy(record_key, state, now_secs(), false) {
        tracing::warn!(%error, "LogonPolicy couldn't be recorded");
    }
}
