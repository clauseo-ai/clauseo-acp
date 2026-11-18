//! A lawyer's file the rename may not replace is written in place ([`crate::write`]'s
//! `in_place`), only once the history's copy of what it holds is confirmed, never through a
//! second name; a file refused either way is named, and the files after it are still written
//! ([`super::renames`]). On Linux the refusal is simulated ("rename refused"); on macOS one test
//! has the real one, a file whose ACL denies `delete`.

use std::fmt::Write as _;
use std::os::unix::fs::MetadataExt as _;
use std::path::PathBuf;

use super::Op;
use super::outcome_tests::{Matter, blob, matter, path, put_text, write_back};
use crate::history::HistoryStore as _;
use crate::stat::{CacheEntry, EntryState, Stat};
use crate::{Error, OsProbe, RefusedBecause, write};

/// Puts `text` in the matter's file `name`, stores it in the history and records it in the
/// cache as the last checkpoint's: what the caller's `outside-edits` checkpoint leaves.
pub(super) fn saved(matter: &mut Matter, name: &str, text: &str) -> PathBuf {
    const PROBE: &dyn crate::PlaceholderProbe = &OsProbe;
    let file = matter.folder.join(name);
    put_text(&file, text);
    let len = u64::try_from(text.len()).unwrap();
    let guard = crate::no_download::NoDownload::begin(PROBE);
    let (oid, _) = matter
        .core
        .history
        .store_file(&file, len, u64::MAX, &guard)
        .unwrap();
    let stat = Stat::from_metadata(&std::fs::symlink_metadata(&file).unwrap());
    matter.core.cache.entries.push(CacheEntry {
        path: path(name),
        stat,
        state: EntryState::Read(oid),
    });
    file
}

/// Records the stat the file `name` (entry `at` of the cache) has now, as the checkpoint
/// before a write-back would after a change to its metadata (a mode, a link, an ACL).
pub(super) fn record_stat(matter: &mut Matter, at: usize, name: &str) {
    let file = matter.folder.join(name);
    matter.core.cache.entries[at].stat =
        Stat::from_metadata(&std::fs::symlink_metadata(&file).unwrap());
}

fn read(file: &std::path::Path) -> String {
    std::fs::read_to_string(file).unwrap()
}

/// Writes `ops` with every rename refused, as macOS refuses one over a file denying `delete`.
pub(super) fn refused_write_back(
    matter: &Matter,
    ops: &[Op],
) -> Result<super::Deleted, super::Failed> {
    write::fail_at(Some("rename refused"));
    let done = write_back(matter, ops);
    write::fail_at(None);
    done
}

/// A refused rename: the file is written in place, the same inode with its mode, the new
/// content, nothing refused and no temp file left.
#[test]
fn a_refused_rename_writes_the_file_in_place() {
    use std::os::unix::fs::PermissionsExt as _;
    let mut matter = matter("in-place-written");
    let file = saved(&mut matter, "Lease.docx", "lease now, longer than then");
    write::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
    record_stat(&mut matter, 0, "Lease.docx");
    let inode = std::fs::metadata(&file).unwrap().ino();
    let ops = [Op::Write(
        path("Lease.docx"),
        blob(&mut matter, "lease then"),
    )];
    let done = refused_write_back(&matter, &ops).unwrap();
    assert!(done.unfinished.refused.is_empty(), "{:?}", done.unfinished);
    assert_eq!(read(&file), "lease then");
    let metadata = std::fs::metadata(&file).unwrap();
    assert_eq!(metadata.ino(), inode);
    assert_eq!(metadata.permissions().mode() & 0o777, 0o640);
    assert_eq!(
        write::temps_in(&matter.folder),
        Vec::<std::ffi::OsString>::new()
    );
}

/// A refused rename of a file with a second name (a hard link outside the matter): never
/// written in place, which would change the other file too; named, both as they were.
#[test]
fn a_hard_linked_target_is_refused_and_left_as_it_was() {
    let mut matter = matter("in-place-hard-link");
    let file = saved(&mut matter, "Lease.docx", "lease now");
    let elsewhere = matter.folder.with_file_name("elsewhere.docx");
    write::hard_link(&file, &elsewhere).unwrap();
    record_stat(&mut matter, 0, "Lease.docx");
    let ops = [Op::Write(
        path("Lease.docx"),
        blob(&mut matter, "lease then"),
    )];
    let done = refused_write_back(&matter, &ops).unwrap();
    assert_eq!(
        done.unfinished.refused,
        [(path("Lease.docx"), RefusedBecause::SecondName)]
    );
    assert_eq!(read(&file), "lease now");
    assert_eq!(read(&elsewhere), "lease now");
}

/// Three files, the middle one refused (its saved copy not confirmed): the other two are
/// written, the middle one is named and as it was, and the error the caller gives says so.
#[test]
fn a_refused_file_between_two_others_is_named_and_the_others_written() {
    let mut matter = matter("in-place-middle");
    let a = saved(&mut matter, "a.docx", "a now");
    let b = saved(&mut matter, "b.docx", "b now");
    let c = saved(&mut matter, "sub/c.docx", "c now");
    // b's entry: not in the last checkpoint's tree, so nothing confirms its saved copy.
    matter.core.cache.entries[1].state = EntryState::Skipped;
    let ops = [
        Op::Write(path("a.docx"), blob(&mut matter, "a then")),
        Op::Write(path("b.docx"), blob(&mut matter, "b then")),
        Op::Write(path("sub/c.docx"), blob(&mut matter, "c then")),
    ];
    let done = refused_write_back(&matter, &ops).unwrap();
    assert_eq!(
        (read(&a), read(&b), read(&c)),
        ("a then".into(), "b now".into(), "c then".into())
    );
    let refused = [(path("b.docx"), RefusedBecause::SavedCopyUnconfirmed)];
    assert_eq!(done.unfinished.refused, refused);
    let ops: Vec<&Op> = ops.iter().collect();
    let error = done.unfinished.refused_error(&ops);
    assert!(
        matches!(&error, Some(Error::FilesRefused { files, of: 3 }) if files == &refused),
        "{error:?}"
    );
}

/// The saved copy unreadable (its loose object gone from the history): the file is never
/// opened for the write, it keeps every byte, and it is named.
#[test]
fn an_unreadable_saved_copy_stops_the_write_before_any_byte() {
    let mut matter = matter("in-place-unreadable");
    let file = saved(&mut matter, "Lease.docx", "lease now");
    let new = blob(&mut matter, "lease then");
    let objects = matter.folder.with_file_name("root/m/history.git/objects");
    let saved_blob = matter.core.cache.entries[0].state.blob().unwrap();
    let hex = saved_blob.0.iter().fold(String::new(), |mut hex, byte| {
        write!(hex, "{byte:02x}").unwrap();
        hex
    });
    let (fan, rest) = hex.split_at(2);
    write::remove_file(&objects.join(fan).join(rest)).unwrap();
    let ops = [Op::Write(path("Lease.docx"), new)];
    let done = refused_write_back(&matter, &ops).unwrap();
    assert_eq!(
        done.unfinished.refused,
        [(path("Lease.docx"), RefusedBecause::SavedCopyUnconfirmed)]
    );
    assert_eq!(read(&file), "lease now");
}

/// macOS: a file whose ACL denies `delete` (`chmod +a "everyone deny delete"`): the rename over
/// it is refused for real, and it is written in place, keeping its inode and its ACL.
#[cfg(target_os = "macos")]
#[test]
fn a_deny_delete_file_is_written_in_place_keeping_its_acl_on_macos() {
    let mut matter = matter("in-place-deny-delete");
    let file = saved(&mut matter, "a4.docx", "a4 now");
    write::deny_everyone_delete(&file).unwrap();
    record_stat(&mut matter, 0, "a4.docx");
    let acl = |file: &std::path::Path| write::acl_text(&std::fs::File::open(file).unwrap());
    let before = acl(&file);
    assert!(before.contains(":deny:delete"), "{before}");
    let inode = std::fs::metadata(&file).unwrap().ino();
    let ops = [Op::Write(path("a4.docx"), blob(&mut matter, "a4 then"))];
    let done = write_back(&matter, &ops).unwrap();
    assert!(done.unfinished.refused.is_empty(), "{:?}", done.unfinished);
    assert_eq!(read(&file), "a4 then");
    assert_eq!(std::fs::metadata(&file).unwrap().ino(), inode);
    assert_eq!(acl(&file), before);
    write::clear_acl(&file).unwrap();
}
