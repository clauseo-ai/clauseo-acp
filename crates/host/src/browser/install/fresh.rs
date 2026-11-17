//! The download's guardrails (decision record 0029, "Never older, never stale"): which version
//! may be installed, which installed copy may run, and when an old copy may go. Pure but for
//! [`installed`], [`read_record`] and [`write_record`], which touch only `<app data>/browsers`.
//!
//! - **Never older.** No version below [`CFT_FLOOR`], the floor built into this release, is
//!   installed or run, and no version below the highest one installed is installed
//!   ([`check_install`]). The search runs the highest installed copy at or above the floor
//!   (`chrome::find`), so an older copy left on disk never runs.
//! - **Never stale.** Each check of Google's list (at the download, and daily while the app runs)
//!   updates [`StableRecord`] in `browsers/stable.json`: the Stable version seen, and the one
//!   before it. A copy older than the Stable before the current one is more than one Stable
//!   security release behind ([`too_far_behind`]): no session starts it, the app tells the
//!   lawyer, and the new one downloads first. When the record had not been refreshed for
//!   [`STALE_AFTER_SECS`] before a new Stable showed, the releases in between are unknown, so
//!   the earlier Stable is forgotten and any copy older than Stable counts as too far behind.
//! - **Old copies** go once a newer one is in place and no session runs from them
//!   (`install::prune`).
//!
//! The record is the host's own file, in a folder no agent can write (the fences); a record
//! that is missing or does not parse refuses nothing, so a copy installed before it existed
//! still runs until the next check writes one.

use std::path::{Path, PathBuf};

use clauseo_protocol::BrowserInstallFailure;
use serde::{Deserialize, Serialize};

use super::Failure;
use crate::browser::chrome::{self, CFT_PREFIX};

/// The oldest Chrome for Testing this release installs or runs: Stable's build line on the
/// day this floor was last raised (157.0.8103.78 was Stable on 2026-11-15). Raise it with each
/// release that ships after a Chrome security release; never lower it.
pub const CFT_FLOOR: &str = "157.0.8103.0";

/// The record's file name in `<app data>/browsers`.
pub const RECORD_FILE: &str = "stable.json";

/// How old the record may be when a new Stable shows and the one before it still counts as
/// "one release behind": Chrome ships a Stable security release about every week, so after two
/// weeks unseen, more than one may have come and gone.
pub const STALE_AFTER_SECS: i64 = 14 * 24 * 60 * 60;

/// What the checks of Google's list have seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StableRecord {
    /// The Stable version at the last check.
    pub stable: String,
    /// The Stable version before that one, while known: `None` at the first check, or when a
    /// new Stable showed after the record went unrefreshed for [`STALE_AFTER_SECS`].
    pub previous: Option<String>,
    /// When the list was last read, in seconds since the Unix epoch, on this computer's clock.
    pub checked_at: i64,
}

/// `text` as numbers to compare, or `None` when it is not a dotted version.
fn parse(text: &str) -> Option<Vec<u64>> {
    chrome::version(text)
}

/// Whether `version` is at or above [`CFT_FLOOR`]; a version that does not parse is not.
pub fn at_or_above_floor(version: &str) -> bool {
    match (parse(version), parse(CFT_FLOOR)) {
        (Some(version), Some(floor)) => version >= floor,
        _ => false,
    }
}

/// Whether Stable's `stable` may be installed over the copies `installed` (their versions):
/// `Err(host_refused)` below the floor or below the highest installed; `Ok(false)` when the
/// highest installed is `stable` already (nothing to download); `Ok(true)` to download.
pub fn check_install(stable: &str, installed: &[String]) -> Result<bool, Failure> {
    let refused = |message: String| Failure::new(BrowserInstallFailure::HostRefused, message);
    let Some(wanted) = parse(stable) else {
        return Err(refused(format!(
            "the version {stable:?} is not dotted digits"
        )));
    };
    if !at_or_above_floor(stable) {
        return Err(refused(format!(
            "Stable {stable} is older than this release's floor {CFT_FLOOR}"
        )));
    }
    match installed.iter().filter_map(|version| parse(version)).max() {
        Some(highest) if wanted < highest => Err(refused(format!(
            "Stable {stable} is older than the copy installed; never downgraded"
        ))),
        Some(highest) if wanted == highest => Ok(false),
        _ => Ok(true),
    }
}

/// The record after a check at `now` that read Stable `stable`. A Stable older than the one
/// recorded leaves the record as it was but for the time (never downgraded).
pub fn observe(record: Option<&StableRecord>, stable: &str, now: i64) -> StableRecord {
    let Some(record) = record else {
        return StableRecord {
            stable: stable.to_owned(),
            previous: None,
            checked_at: now,
        };
    };
    let newer = match (parse(stable), parse(&record.stable)) {
        (Some(seen), Some(had)) => seen > had,
        (Some(_), None) => true,
        _ => false,
    };
    if !newer {
        return StableRecord {
            checked_at: now,
            ..record.clone()
        };
    }
    let fresh = now.saturating_sub(record.checked_at) <= STALE_AFTER_SECS;
    StableRecord {
        stable: stable.to_owned(),
        previous: fresh.then(|| record.stable.clone()),
        checked_at: now,
    }
}

/// Whether the copy of `version` is more than one Stable security release behind `record`: older
/// than Stable and older than the Stable before it (or than Stable alone, when the one before is
/// unknown). No record refuses nothing.
pub fn too_far_behind(version: &str, record: Option<&StableRecord>) -> bool {
    let Some(record) = record else {
        return false;
    };
    let Some(version) = parse(version) else {
        return true;
    };
    let older = |than: &str| parse(than).is_some_and(|than| version < than);
    if !older(&record.stable) {
        return false;
    }
    record.previous.as_deref().is_none_or(older)
}

/// The record in `browsers`, or `None` when it is missing, not a regular file, or unreadable.
pub fn read_record(browsers: &Path) -> Option<StableRecord> {
    let path = browsers.join(RECORD_FILE);
    let file = crate::write::open_read_nofollow(&path).ok()?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(file, 4096), &mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Writes `record` to `browsers`, atomically, replacing a link there rather than following it.
pub fn write_record(browsers: &Path, record: &StableRecord) -> std::io::Result<()> {
    let bytes = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    crate::write::replace_private_file(&browsers.join(RECORD_FILE), &bytes)
}

/// The working copies in `browsers`: those of [`installed`] whose `binary` (relative to the copy's
/// folder) is a regular file there, highest version last. A folder without it (a broken copy,
/// which the search passes over too) neither blocks a download nor stays as the newest.
pub fn working(browsers: &Path, binary: &Path) -> Vec<(String, PathBuf)> {
    installed(browsers)
        .into_iter()
        .filter(|(_, folder)| {
            std::fs::symlink_metadata(folder.join(binary)).is_ok_and(|meta| meta.is_file())
        })
        .collect()
}

/// The copies in `browsers`: each real `chrome-<version>` folder's version and path, highest
/// version last. Links and other names are passed over.
pub fn installed(browsers: &Path) -> Vec<(String, PathBuf)> {
    let mut found: Vec<(Vec<u64>, String, PathBuf)> = std::fs::read_dir(browsers)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            if !entry.file_type().ok()?.is_dir() {
                return None;
            }
            let name = entry.file_name();
            let version = name.to_str()?.strip_prefix(CFT_PREFIX)?.to_owned();
            Some((parse(&version)?, version, entry.path()))
        })
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|(_, version, path)| (version, path))
        .collect()
}

#[cfg(test)]
mod tests;
