//! The backups a crashed rewrite left beside the lawyer's files. A rewrite on Windows
//! (`write::replace_keeping`) has `ReplaceFileW` move the old file to `<name>.clauseo-<n>.orig.tmp`
//! on the way; a crash between the call's two renames leaves `<name>` missing with the old file
//! under that name. The name ends in `.tmp`, so no checkpoint versions it and the papers list
//! never shows it, and it ends in `.orig.tmp`, not the `.clauseo-*.new` the host's temp sweep
//! takes, so no sweep removes it.
//!
//! A name too long to take that suffix within the 255 UTF-16 units a folder allows a name (over
//! 236 units) gets `.clauseo-<hash>-<n>.orig.tmp` instead, `<hash>` being 16 hex digits of a fixed
//! hash of the name ([`name_hash`]): without it the call cannot run, and the rewrite would lose
//! the file's creation time, attributes and `Zone.Identifier` stream. Such a backup is matched
//! to its file through the names the stat cache holds in its folder, those too long for the
//! suffix: exactly one whose hash it is, or the backup is left alone and counted, never put
//! back on a guess.
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
//! worker (the matter's one writer, `matter/leftovers.rs`); `Matter::open` itself reads nothing
//! in the folder. Built in tests on every OS. Nothing here fails; the counts are logged once.

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Matter;
use super::leftovers::folders;
use crate::checkpoint::Core;
use crate::history::HistoryStore as _;
use crate::no_download::NoDownload;
use crate::{PlaceholderProbe, fsync, write};

/// What sits between the target's name and the number, without the dot.
const MARK: &str = "clauseo-";

/// The most UTF-16 units a name in a folder may have (NTFS; `ReFS`, exFAT and FAT32 too).
const NAME_MAX: usize = 255;

/// The backup name of the file `name`: `<name>.clauseo-<n>.orig.tmp`, or, when that would pass
/// [`NAME_MAX`], `.clauseo-<hash>-<n>.orig.tmp` with [`name_hash`] in 16 hex digits.
#[cfg(any(windows, test))]
pub(crate) fn backup_name(name: &OsStr, n: u32) -> std::ffi::OsString {
    if too_long(name, n) {
        return format!(".{MARK}{:016x}-{n}.orig.tmp", name_hash(name)).into();
    }
    let mut backup = name.to_owned();
    backup.push(suffix(n));
    backup
}

/// The suffix of backup `n`: `.clauseo-<n>.orig.tmp`.
fn suffix(n: u32) -> String {
    format!(".{MARK}{n}.orig.tmp")
}

/// Whether `name` with backup `n`'s suffix passes [`NAME_MAX`]. Counted in UTF-16 units, as
/// NTFS counts: exact on Windows too, where an unpaired surrogate is one unit, as is the
/// replacement character it is read as.
fn too_long(name: &OsStr, n: u32) -> bool {
    name.to_string_lossy().encode_utf16().count() + suffix(n).len() > NAME_MAX
}

/// A 64-bit FNV-1a hash of `name`'s bytes (its UTF-8, the same on every OS for a name that is
/// valid Unicode, as every name the stat cache holds is). Fixed, never std's `DefaultHasher`:
/// the build that recovers a backup may be newer than the one that named it.
fn name_hash(name: &OsStr) -> u64 {
    name.as_encoded_bytes()
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
}

/// The hash and the number of a backup named `.clauseo-<16 hex digits>-<digits>.orig.tmp`, if
/// `name` is one ([`backup_name`] for a name too long for the suffix).
fn hashed_of(name: &OsStr) -> Option<(u64, u32)> {
    let rest = name.to_str()?.strip_prefix('.')?.strip_prefix(MARK)?;
    let (hash, n) = rest.strip_suffix(".orig.tmp")?.split_once('-')?;
    let hex = hash.len() == 16
        && hash
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
    let digits = !n.is_empty() && n.bytes().all(|byte| byte.is_ascii_digit());
    if !hex || !digits {
        return None;
    }
    Some((u64::from_str_radix(hash, 16).ok()?, n.parse().ok()?))
}

/// The one name of `cached` (the stat cache's names in a folder) that a backup hashed `hash`,
/// numbered `n`, can be of: too long for the suffix, and of that hash. None, or more than one,
/// gives none.
fn hashed_target(cached: &[String], hash: u64, n: u32) -> Option<&str> {
    let mut found = cached.iter().map(String::as_str).filter(|name| {
        let name = OsStr::new(name);
        too_long(name, n) && name_hash(name) == hash
    });
    let first = found.next()?;
    found.next().is_none().then_some(first)
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
    /// Backups whose content the store does not hold, or hashed ones matched to no one file,
    /// left in place.
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
/// `matter/backups.rs`). Windows only, through `recover_leftovers` as the host's matter worker
/// opens the matter; built on every OS for tests.
pub fn recover_backups(matter: &mut Matter) {
    let probe = matter.settings.probe.as_ref();
    recover(&mut matter.core, &matter.folder, probe);
}

/// [`recover_backups`] of `folder`.
fn recover(core: &mut Core, folder: &Path, probe: &dyn PlaceholderProbe) {
    let mut done = Recovered::default();
    for (dir, cached) in folders(core, folder) {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut by_target: BTreeMap<PathBuf, Vec<Found>> = BTreeMap::new();
        for entry in entries.flatten() {
            let name = entry.file_name();
            let target = match (target_of(&name), hashed_of(&name)) {
                (Some(target), _) => dir.join(target),
                (None, Some((hash, n))) => {
                    let Some(target) = hashed_target(&cached, hash, n) else {
                        done.kept += 1;
                        continue;
                    };
                    dir.join(target)
                }
                (None, None) => continue,
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

    use super::{backup_name, hashed_of, hashed_target, name_hash, target_of};

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

    /// The fixed hash: FNV-1a's own test value, and the value of a 240-unit name, which a
    /// backup a crash left must keep in every later build.
    #[test]
    fn the_name_hash_never_changes() {
        assert_eq!(name_hash(OsStr::new("a")), 0xaf63_dc4c_8601_ec8c);
        let name = format!("{}.docx", "a".repeat(235));
        assert_eq!(name_hash(OsStr::new(&name)), 0x6f79_de2a_a128_2bde);
    }

    /// Up to 236 units a name takes the suffix (255 in all); from 237, or at a number with more
    /// digits, it gets the short hashed name, which gives back its hash and number and is no
    /// `<name>.clauseo-<n>.orig.tmp`.
    #[test]
    fn a_name_too_long_for_the_suffix_gets_a_hashed_backup_name() {
        let named = |len: usize| format!("{}.docx", "a".repeat(len - 5));
        let name = named(236);
        let backup = backup_name(OsStr::new(&name), 0);
        assert_eq!(backup, format!("{name}.clauseo-0.orig.tmp").as_str());
        assert_eq!(backup.len(), 255);
        let backup = backup_name(OsStr::new(&name), 10);
        assert_eq!(backup.to_str().unwrap().len(), 37);
        for len in [237, 240, 255] {
            let name = named(len);
            let backup = backup_name(OsStr::new(&name), 3);
            let hash = name_hash(OsStr::new(&name));
            assert_eq!(backup, format!(".clauseo-{hash:016x}-3.orig.tmp").as_str());
            assert_eq!(hashed_of(&backup), Some((hash, 3)));
            assert_eq!(target_of(&backup), None);
        }
        // Not hashed names.
        for other in [
            "Lease.docx.clauseo-0.orig.tmp",
            ".clauseo-3.orig.tmp",
            ".clauseo-6f79de2aa1282bde-.orig.tmp",
            ".clauseo-6f79de2aa1282bd-1.orig.tmp",
            ".clauseo-6F79DE2AA1282BDE-1.orig.tmp",
            ".clauseo-6f79de2aa1282bde-1.orig.tmp.tmp",
            "x.clauseo-6f79de2aa1282bde-1.orig.tmp",
        ] {
            assert_eq!(hashed_of(OsStr::new(other)), None, "{other}");
        }
    }

    /// A name is measured in UTF-16 units, as NTFS counts: a character outside the BMP is two
    /// (a surrogate pair), so one ending at unit 236 takes the suffix and one ending at unit 237
    /// gets the hashed name, though both are well over 236 bytes in UTF-8.
    #[test]
    fn a_surrogate_pair_at_the_236th_unit_counts_as_two_units() {
        // 229 + 2 + 5 = 236 units, 240 bytes.
        let fits = format!("{}\u{1F4DC}.docx", "a".repeat(229));
        assert_eq!(fits.encode_utf16().count(), 236);
        let backup = backup_name(OsStr::new(&fits), 0);
        assert_eq!(backup, format!("{fits}.clauseo-0.orig.tmp").as_str());
        assert_eq!(backup.to_string_lossy().encode_utf16().count(), 255);
        assert_eq!(target_of(&backup), Some(OsStr::new(&fits)));
        // 230 + 2 + 5 = 237 units: the pair straddles units 236 and 237.
        let over = format!("{}\u{1F4DC}.docx", "a".repeat(230));
        assert_eq!(over.encode_utf16().count(), 237);
        let backup = backup_name(OsStr::new(&over), 0);
        let hash = name_hash(OsStr::new(&over));
        assert_eq!(backup, format!(".clauseo-{hash:016x}-0.orig.tmp").as_str());
        assert_eq!(
            hashed_target(std::slice::from_ref(&over), hash, 0),
            Some(over.as_str())
        );
    }

    /// A hashed backup is of the one cached name too long for the suffix with its hash: never a
    /// short name, and none when two match.
    #[test]
    fn a_hashed_backup_is_of_exactly_one_cached_name() {
        let long = format!("{}.docx", "a".repeat(235));
        let hash = name_hash(OsStr::new(&long));
        let cached = vec!["Lease.docx".to_owned(), long.clone()];
        assert_eq!(hashed_target(&cached, hash, 0), Some(long.as_str()));
        assert_eq!(hashed_target(&cached, hash ^ 1, 0), None);
        // A short name of the hash is never the target.
        let short = vec!["Lease.docx".to_owned()];
        let short_hash = name_hash(OsStr::new("Lease.docx"));
        assert_eq!(hashed_target(&short, short_hash, 0), None);
        // Two names of one hash (here the same one twice): none.
        let twice = vec![long.clone(), long];
        assert_eq!(hashed_target(&twice, hash, 0), None);
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
