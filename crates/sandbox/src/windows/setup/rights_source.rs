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

use super::ffi::{reg_last_write, reg_string, reg_subkeys, windows_dir};
use super::machine::MachineError;
use super::preflight::holders;
use super::preflight::sid_of_name;
use super::record_logon;
use super::record_write::{self, RECORD_KEY};
use super::result_file::now_secs;
use super::rights_policy::{
    CACHE_TTL, INTERACTIVE_RIGHT, LaunchRecord, MDM_KEY, MDM_PROVIDERS, PolicyCache, PolicySource,
    PolicyState, Pool, RightPolicy, provider_key, read_policy, update_launch_record,
};
use crate::windows::pool::MachineRecord;
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

    fn mdm_keys(&mut self) -> io::Result<Vec<(String, u64)>> {
        let mut keys = Vec::new();
        let providers = reg_subkeys(MDM_PROVIDERS)
            .map_err(io::Error::other)?
            .unwrap_or_default();
        let candidates =
            std::iter::once(MDM_KEY.to_owned()).chain(providers.iter().map(|p| provider_key(p)));
        for key in candidates {
            if let Some(stamp) = reg_last_write(&key).map_err(io::Error::other)? {
                keys.push((key, stamp));
            }
        }
        Ok(keys)
    }

    fn mdm_value(&mut self, key: &str, name: &str) -> io::Result<Option<String>> {
        reg_string(key, name).map_err(io::Error::other)
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

/// The record key as [`LaunchRecord`] reads and writes it.
struct RegistryRecord<'a> {
    key: &'a str,
}

impl LaunchRecord for RegistryRecord<'_> {
    fn read(&mut self) -> Result<Option<MachineRecord>, MachineError> {
        record_write::read(self.key)
    }

    fn policy(&mut self, pool: Pool<'_>) -> RightPolicy {
        read_now(INTERACTIVE_RIGHT, pool)
    }

    fn holders(&mut self) -> Result<Vec<Sid>, MachineError> {
        holders(INTERACTIVE_RIGHT)
    }

    fn write(&mut self, state: PolicyState, at: u64) -> Result<(), MachineError> {
        record_logon::write_logon_policy(self.key, state, at, false)
    }
}

/// The SYSTEM refresh's update of the record's `LogonPolicy` (`refresh::locked_steps`, under
/// the set-up lock, after its checks): [`update_launch_record`] on the machine's record, so a
/// policy that changed since set-up names itself at a launch's 1385. A failure is logged.
pub fn record_for_launches() {
    let mut record = RegistryRecord { key: RECORD_KEY };
    match update_launch_record(&mut record, now_secs()) {
        Ok(update) => tracing::debug!(?update, "LogonPolicy"),
        Err(error) => tracing::warn!(%error, "LogonPolicy couldn't be recorded"),
    }
}
