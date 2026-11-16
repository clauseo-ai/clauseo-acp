//! The updater's files in the data folder (decision record 0037): `update.json`, the marker
//! written just before an install and read at the next start, and last run's download under
//! `updates/<version>/`. The paths come from `clauseo_settings::Locations`; the host joins none
//! of its own.
//!
//! Every function here touches the disk, so the actor never calls one: they run on the
//! updater's file thread (`actor/update/files.rs`). The log lines name no path (0047): an I/O
//! error is logged by its kind, never its text.

use std::io;
use std::path::PathBuf;

use clauseo_settings::{Files, Locations, SystemFiles};

use super::{CachedDownload, UpdateMarker};

/// `update.json` as the last run left it: `None` when there is none, and also when it is
/// empty, cannot be read or does not parse (a line in the log says which, with no path).
pub fn read_update_marker(locations: &Locations) -> Option<UpdateMarker> {
    let bytes = match std::fs::read(locations.update_marker()) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return None,
        Err(error) => {
            tracing::warn!(kind = %error.kind(), "update.json was not read");
            return None;
        }
    };
    if bytes.iter().all(u8::is_ascii_whitespace) {
        tracing::warn!("update.json is empty; it is ignored");
        return None;
    }
    match serde_json::from_slice(&bytes) {
        Ok(marker) => Some(marker),
        Err(error) => {
            // serde_json's message quotes no path, only a line and column; the category is
            // enough to tell a cut file from a foreign one.
            tracing::warn!(category = ?error.classify(), "update.json does not parse; it is ignored");
            None
        }
    }
}

/// Writes `marker` to `update.json` whole: a temporary file beside it, synced, then a rename, so
/// a reader sees the old file or the new one (`Files::write_atomic`, as `settings.json`); then
/// the data folder is synced, so the rename itself survives a power cut during the install.
/// Once the rename has happened the marker is on disk, so a failed sync is logged by its kind
/// and is not the write's failure: reporting one would count a failed install the next start
/// counts again.
pub(crate) fn write_marker(locations: &Locations, marker: &UpdateMarker) -> io::Result<()> {
    let bytes = serde_json::to_vec(marker).map_err(io::Error::other)?;
    SystemFiles.write_atomic(&locations.update_marker(), &bytes)?;
    if let Err(error) = crate::write::sync_dir(locations.data_dir()) {
        tracing::warn!(kind = %error.kind(), "the data folder was not synced after update.json");
    }
    Ok(())
}

/// Deletes `update.json`; one already gone is fine.
pub(crate) fn clear_marker(locations: &Locations) -> io::Result<()> {
    match crate::write::remove_link(&locations.update_marker()) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Last run's download: the newest `updates/<version>/` folder, of a version newer than the
/// running one, that holds exactly one file, the installer the desktop's updater saved there.
/// Anything else (a folder with no file or several, a name that is not a version, an older
/// version) is not a download this host can offer again.
pub(crate) fn find_cached(locations: &Locations, current: &str) -> Option<CachedDownload> {
    let running = version_key(current);
    let entries = std::fs::read_dir(locations.updates_dir()).ok()?;
    entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let version = entry.file_name().into_string().ok()?;
            let key = version_key(&version)?;
            let newer = running
                .as_ref()
                .map_or(version != current, |running| key > *running);
            if !newer {
                return None;
            }
            let path = only_file(entry.path())?;
            Some(CachedDownload { version, path })
        })
        .max_by(|a, b| version_key(&a.version).cmp(&version_key(&b.version)))
}

/// Removes what `updates/` holds besides `keep`'s folder: older runs' downloads, a version's
/// folder with no file or several, anything else. Each failure is logged by its kind, and the
/// rest go on.
pub(crate) fn remove_old_downloads(locations: &Locations, keep: Option<&CachedDownload>) {
    let Ok(entries) = std::fs::read_dir(locations.updates_dir()) else {
        return;
    };
    let kept = keep.and_then(|cached| cached.path.parent());
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if Some(path.as_path()) == kept {
            continue;
        }
        let folder = entry.file_type().is_ok_and(|kind| kind.is_dir());
        let removed = if folder {
            crate::write::remove_tree(&path)
        } else {
            crate::write::remove_link(&path)
        };
        if let Err(error) = removed {
            tracing::warn!(kind = %error.kind(), "an old update download was not removed");
        }
    }
}

/// The one regular file in `folder`, when there is exactly one.
fn only_file(folder: PathBuf) -> Option<PathBuf> {
    let mut files = std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| entry.path());
    let first = files.next()?;
    files.next().is_none().then_some(first)
}

/// `0.4.1` as `[0, 4, 1]`, for ordering versions; `None` for a name that is not dotted numbers
/// (a pre-release suffix is not a version this updater writes).
fn version_key(version: &str) -> Option<Vec<u64>> {
    version
        .split('.')
        .map(|part| part.parse::<u64>().ok())
        .collect()
}

#[cfg(test)]
mod tests;
