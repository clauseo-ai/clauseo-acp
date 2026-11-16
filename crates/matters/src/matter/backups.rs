//! The backups a crashed rewrite left beside the lawyer's files. A rewrite on Windows
//! (`write::replace_keeping`) has `ReplaceFileW` move the old file to `<name>.clauseo-<n>.orig.tmp`
//! on the way; a crash between the call's two renames leaves `<name>` missing with the old file
//! under that name. The name ends in `.tmp`, so no checkpoint versions it and the papers list
//! never shows it, and it ends in `.orig.tmp`, not the `.clauseo-*.new` the host's temp sweep
//! takes, so no sweep removes it.
//!
//! [`recover_backups`] goes through them, in every folder the stat cache has a file in (a
//! crashed rewrite's target was in it: the write checks the cache's stat first). Only a backup
//! whose content the shadow store holds is acted on: a real one always is (the write checks the
//! target against the cache before the call), while one an agent wrote, or a stray kept from an
//! earlier open, may not be, and putting that back would bring in a file outside any turn.
//! - `<name>` missing: the held backup is renamed back to it. A backup whose target is missing
//!   is never removed.
//! - `<name>` there: a held backup is a stray (the rewrite went through, or a later write
//!   replaced the file), and is removed.
//! - Not held: left where it is, and counted.
//!
//! With several backups of one name, the held ones go first, newest first: the first goes back,
//! and the others are then strays. Names are compared as the OS gives them, UTF-8 or not.
//!
//! Only on Windows, the one OS that makes backups, and only from the open of the host's matter
//! worker (the matter's one writer); `Matter::open` itself reads nothing in the folder. Built in
//! tests on every OS. Nothing here fails; the counts are logged once.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Matter;
use crate::checkpoint::Core;
use crate::history::HistoryStore as _;
use crate::no_download::NoDownload;
use crate::{PlaceholderProbe, fsync, write};

/// What sits between the target's name and the number, without the dot.
const MARK: &str = "clauseo-";

/// The backup name of the file `name`: `<name>.clauseo-<n>.orig.tmp`.
#[cfg(any(windows, test))]
pub(crate) fn backup_name(name: &OsStr, n: u32) -> std::ffi::OsString {
    let mut backup = name.to_owned();
    backup.push(format!(".{MARK}{n}.orig.tmp"));
    backup
}

/// The name of the file whose backup is named `name`, if it is a backup name. Taken apart at its
/// dots, so a name that is not UTF-8 keeps its bytes.
pub(crate) fn target_of(name: &OsStr) -> Option<&OsStr> {
    let ends = |path: &Path, ext: &str| path.extension().is_some_and(|it| it == ext);
    let name = Path::new(name);
    let orig = Path::new(name.file_stem().filter(|_| ends(name, "tmp"))?);
    let numbered = Path::new(orig.file_stem().filter(|_| ends(orig, "orig"))?);
    let n = numbered.extension()?.to_str()?.strip_prefix(MARK)?;
    let digits = !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit());
    numbered.file_stem().filter(|_| digits)
}

/// What [`recover_backups`] did, by count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct Recovered {
    /// Backups renamed back to their missing targets.
    put_back: u32,
    /// Strays whose content the store holds, removed.
    removed: u32,
    /// Backups whose content the store does not hold, left in place.
    kept: u32,
    /// Backups a rename, a removal or a stat failed on, left in place.
    failed: u32,
}

/// One backup found: its path, whether the store holds its content, and when it was written.
struct Found {
    path: PathBuf,
    held: bool,
    modified: Option<SystemTime>,
}

/// Puts back or clears the backups crashed rewrites left in the folder of `matter` (see
/// `matter/backups.rs`). Windows only, from the host's matter worker as it opens the matter;
/// built on every OS for tests.
pub fn recover_backups(matter: &mut Matter) {
    let probe = matter.settings.probe.as_ref();
    recover(&mut matter.core, &matter.folder, probe);
}

/// [`recover_backups`] of `folder`.
fn recover(core: &mut Core, folder: &Path, probe: &dyn PlaceholderProbe) {
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
        let mut by_target: BTreeMap<PathBuf, Vec<Found>> = BTreeMap::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let Some(target) = target_of(&name) else {
                continue;
            };
            let path = entry.path();
            let modified = entry.metadata().and_then(|it| it.modified()).ok();
            let held = in_store(core, &path, probe);
            let found = Found {
                path,
                held,
                modified,
            };
            by_target.entry(dir.join(target)).or_default().push(found);
        }
        for (target, mut found) in by_target {
            // Held first, newest first.
            found.sort_by(|a, b| b.held.cmp(&a.held).then(b.modified.cmp(&a.modified)));
            for backup in found {
                one(&dir, &backup, &target, &mut done);
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
}

/// One backup, `backup` of `target`, in `dir`.
fn one(dir: &Path, backup: &Found, target: &Path, done: &mut Recovered) {
    if !backup.held {
        done.kept += 1;
        return;
    }
    match std::fs::symlink_metadata(target) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            match write::rename(&backup.path, target).and_then(|()| fsync::sync_dir(dir)) {
                Ok(()) => done.put_back += 1,
                Err(_) => done.failed += 1,
            }
        }
        Ok(_) => match write::remove_file(&backup.path) {
            Ok(()) => done.removed += 1,
            Err(_) => done.failed += 1,
        },
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
        assert_eq!(target_of(&name), Some(OsStr::new("Lease v2.docx")));
        assert_eq!(
            target_of(OsStr::new("a.clauseo-1.clauseo-2.orig.tmp")),
            Some(OsStr::new("a.clauseo-1"))
        );
        assert_eq!(
            target_of(OsStr::new("Lease.clauseo-0.orig.tmp")),
            Some(OsStr::new("Lease"))
        );
        for other in [
            "Lease.docx",
            ".clauseo-3.tmp",
            ".clauseo-3.orig.tmp",
            "Lease.docx.clauseo-.orig.tmp",
            "Lease.docx.clauseo-x.orig.tmp",
            "Lease.docx.clauseo-1.orig.tmp.tmp",
            "Lease.docx.clauseo-1-2.new",
        ] {
            assert_eq!(target_of(OsStr::new(other)), None, "{other}");
        }
    }

    /// A name that is not UTF-8 keeps its bytes.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_gives_back_its_target() {
        use std::os::unix::ffi::OsStrExt as _;
        let target = OsStr::from_bytes(b"Lease \xff.docx");
        let name = backup_name(target, 0);
        assert_eq!(target_of(&name), Some(target));
    }
}
