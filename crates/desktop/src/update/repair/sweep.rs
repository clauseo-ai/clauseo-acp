//! The start's sweep of last runs' attempts, `updates/<version>/repair-*`, before the host's own
//! clean-up of `updates/` runs. A launched installer runs from its folder after Clauseo quits, so
//! the folder stays until here. The sweep never follows a link: each entry is read with
//! `symlink_metadata`, and a folder that is a link or (Windows) any reparse point, junctions
//! included, is left alone, as is anything in one that is not a plain file. Files are deleted
//! one by one, then the empty folder: no recursive delete. A file that can't be deleted (a
//! running installer's image) leaves its folder for the next start. One host.log line per folder.

#![expect(
    clippy::disallowed_methods,
    reason = "the sweep's deletes of last runs' attempts, file by file; the desktop crate has no \
              write module"
)]

use std::fs::{self, Metadata};
use std::io;
use std::path::Path;

use super::super::install::LIFECYCLE;
use super::super::plugin::is_plain_version;

/// The attempts' folder prefix, under a version's folder.
pub(super) const PREFIX: &str = "repair-";

/// What became of one attempt's folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Swept {
    /// Deleted.
    Removed,
    /// A link or a reparse point: left alone, never followed.
    Link,
    /// It holds something other than plain files: left alone.
    Unexpected,
    /// A file in it could not be deleted (in use): left for the next start.
    InUse,
}

impl Swept {
    fn word(self) -> &'static str {
        match self {
            Swept::Removed => "removed",
            Swept::Link => "a link, left alone",
            Swept::Unexpected => "holds more than files, left alone",
            Swept::InUse => "in use, left for the next start",
        }
    }
}

/// Sweeps every `updates/<version>/repair-*`, ignoring what can't be read. Returns what became of
/// each, for the tests.
pub(crate) fn sweep(updates: &Path) -> Vec<Swept> {
    let mut swept = Vec::new();
    // `updates/` itself first: a link or junction there is never followed either.
    if !fs::symlink_metadata(updates).is_ok_and(|meta| plain_folder(&meta)) {
        return swept;
    }
    let Ok(versions) = fs::read_dir(updates) else {
        return swept;
    };
    for version in versions.filter_map(Result::ok) {
        let name = version.file_name();
        let Some(name) = name.to_str().filter(|name| is_plain_version(name)) else {
            continue;
        };
        let folder = version.path();
        if !fs::symlink_metadata(&folder).is_ok_and(|meta| plain_folder(&meta)) {
            continue;
        }
        let Ok(attempts) = fs::read_dir(&folder) else {
            continue;
        };
        for attempt in attempts.filter_map(Result::ok) {
            let named = attempt
                .file_name()
                .to_str()
                .is_some_and(|n| n.starts_with(PREFIX));
            if !named {
                continue;
            }
            let outcome = sweep_one(&attempt.path());
            tracing::info!(
                target: LIFECYCLE,
                version = name,
                outcome = outcome.word(),
                "repair: last run's installer folder"
            );
            swept.push(outcome);
        }
        // Empty now, or not: the host's clean-up of updates/ takes it either way.
        drop(fs::remove_dir(&folder));
    }
    swept
}

/// One attempt's folder: its plain files deleted, then the folder.
fn sweep_one(folder: &Path) -> Swept {
    match fs::symlink_metadata(folder) {
        Ok(meta) if plain_folder(&meta) => {}
        _ => return Swept::Link,
    }
    let Ok(entries) = fs::read_dir(folder) else {
        return Swept::InUse;
    };
    let mut files = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            return Swept::InUse;
        };
        let path = entry.path();
        match fs::symlink_metadata(&path) {
            Ok(meta) if plain_file(&meta) => files.push(path),
            _ => return Swept::Unexpected,
        }
    }
    for file in files {
        if remove(&file, |path| fs::remove_file(path)).is_err() {
            return Swept::InUse;
        }
    }
    match remove(folder, |path| fs::remove_dir(path)) {
        Ok(()) => Swept::Removed,
        Err(_) => Swept::InUse,
    }
}

/// `delete(path)`, where one already gone is done.
fn remove(path: &Path, delete: impl Fn(&Path) -> io::Result<()>) -> io::Result<()> {
    match delete(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// A folder that is neither a link nor a reparse point.
fn plain_folder(meta: &Metadata) -> bool {
    meta.is_dir() && !meta.file_type().is_symlink() && !reparse_point(meta)
}

/// A file that is neither a link nor a reparse point.
fn plain_file(meta: &Metadata) -> bool {
    meta.is_file() && !meta.file_type().is_symlink() && !reparse_point(meta)
}

/// Windows: `FILE_ATTRIBUTE_REPARSE_POINT`, which a junction has and `is_symlink` does not
/// report. Elsewhere `is_symlink` covers every link.
fn reparse_point(meta: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        let _ = meta;
        false
    }
}
