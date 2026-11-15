//! The backups a crashed rewrite left beside the lawyer's files. A rewrite on Windows
//! (`write::replace_keeping`) has `ReplaceFileW` move the old file to `<name>.clauseo-<n>.orig.tmp`
//! on the way; a crash between the call's two renames leaves `<name>` missing with the old file
//! under that name. The name ends in `.tmp`, so no checkpoint versions it and the papers list
//! never shows it, and it ends in `.orig.tmp`, not the `.clauseo-*.new` the host's temp sweep
//! takes, so no sweep removes it.
//!
//! [`recover`] runs as a matter opens, in every folder the stat cache has a file in (a crashed
//! rewrite's target was in it: the write checks the cache's stat first):
//! - `<name>` missing: the backup is renamed back to it. A backup whose target is missing is
//!   never removed.
//! - `<name>` there: the backup is a stray (the rewrite went through, or a later write replaced
//!   the file). It is removed only when its content is in the shadow store; otherwise it stays.
//!
//! Nothing here fails the open; the counts are logged once.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::checkpoint::Core;
use crate::history::HistoryStore as _;
use crate::no_download::NoDownload;
use crate::{PlaceholderProbe, fsync, write};

/// What sits between the target's name and the number.
const MARK: &str = ".clauseo-";
/// What ends a backup's name.
const SUFFIX: &str = ".orig.tmp";

/// The backup name of the file `name`: `<name>.clauseo-<n>.orig.tmp`.
#[cfg(any(windows, test))]
pub(crate) fn backup_name(name: &std::ffi::OsStr, n: u32) -> std::ffi::OsString {
    let mut backup = name.to_owned();
    backup.push(format!("{MARK}{n}{SUFFIX}"));
    backup
}

/// The name of the file whose backup is named `name`, if it is a backup name.
pub(crate) fn target_of(name: &str) -> Option<&str> {
    let (target, n) = name.strip_suffix(SUFFIX)?.rsplit_once(MARK)?;
    let numbered = !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit());
    (numbered && !target.is_empty()).then_some(target)
}

/// What [`recover`] did, by count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Recovered {
    /// Backups renamed back to their missing targets.
    pub(crate) put_back: u32,
    /// Strays whose content the store holds, removed.
    pub(crate) removed: u32,
    /// Strays whose content the store does not hold, left in place.
    pub(crate) kept: u32,
    /// Backups a rename, a removal or a stat failed on, left in place.
    pub(crate) failed: u32,
}

/// Puts back or clears the backups in `folder` (see the module doc).
pub(crate) fn recover(core: &mut Core, folder: &Path, probe: &dyn PlaceholderProbe) -> Recovered {
    let mut dirs = BTreeSet::new();
    dirs.insert(folder.to_owned());
    for entry in &core.cache.entries {
        if let Some((parent, _)) = entry.path.as_str().rsplit_once('/') {
            dirs.insert(folder.join(parent));
        }
    }
    let mut done = Recovered::default();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            if let Some(target) = name.to_str().and_then(target_of) {
                one(
                    core,
                    &dir,
                    &entry.path(),
                    &dir.join(target),
                    probe,
                    &mut done,
                );
            }
        }
    }
    if done != Recovered::default() {
        tracing::info!(
            put_back = done.put_back,
            removed = done.removed,
            kept = done.kept,
            failed = done.failed,
            "backups of crashed rewrites"
        );
    }
    done
}

/// One backup, `backup` of `target`, in `dir`.
fn one(
    core: &mut Core,
    dir: &Path,
    backup: &Path,
    target: &Path,
    probe: &dyn PlaceholderProbe,
    done: &mut Recovered,
) {
    match std::fs::symlink_metadata(target) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            match write::rename(backup, target).and_then(|()| fsync::sync_dir(dir)) {
                Ok(()) => done.put_back += 1,
                Err(_) => done.failed += 1,
            }
        }
        Ok(_) if in_store(core, backup, probe) => match write::remove_file(backup) {
            Ok(()) => done.removed += 1,
            Err(_) => done.failed += 1,
        },
        Ok(_) => done.kept += 1,
        Err(_) => done.failed += 1,
    }
}

/// Whether the content of the file `backup` is a blob the matter's store holds. Hashed, never
/// stored; a file not downloaded is never read, and counts as not held.
fn in_store(core: &mut Core, backup: &Path, probe: &dyn PlaceholderProbe) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(backup) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    let guard = NoDownload::begin(probe);
    core.history
        .hash_file(backup, metadata.len(), &guard)
        .is_ok_and(|(blob, _)| core.history.has_blob(blob))
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::{backup_name, target_of};

    #[test]
    fn a_backup_name_gives_back_its_target() {
        let name = backup_name(OsStr::new("Lease v2.docx"), 7);
        assert_eq!(name, "Lease v2.docx.clauseo-7.orig.tmp");
        assert_eq!(target_of(name.to_str().unwrap()), Some("Lease v2.docx"));
        assert_eq!(
            target_of("a.clauseo-1.clauseo-2.orig.tmp"),
            Some("a.clauseo-1")
        );
        for other in [
            "Lease.docx",
            ".clauseo-3.tmp",
            ".clauseo-3.orig.tmp",
            "Lease.docx.clauseo-.orig.tmp",
            "Lease.docx.clauseo-x.orig.tmp",
            "Lease.docx.clauseo-1-2.new",
        ] {
            assert_eq!(target_of(other), None, "{other}");
        }
    }
}
