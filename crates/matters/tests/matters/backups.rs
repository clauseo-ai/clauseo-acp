//! The backups a crashed rewrite left (`<name>.clauseo-<n>.orig.tmp`, or
//! `.clauseo-<hash>-<n>.orig.tmp` for a name too long for the suffix), as the host's matter worker
//! opens the matter again (`recover_backups`): a backup whose content the store holds is put
//! back where the file is missing and removed where it is there; any other is left alone.
//! `Matter::open` alone touches none of them.

use clauseo_matters::{Matter, recover_backups};

use crate::fixture::Fixture;
use crate::support;

const BACKUP: &str = "Papers/Lease.docx.clauseo-0.orig.tmp";

/// `Papers/Lease.docx` "old" imported, the matter closed.
fn imported(name: &str) -> Fixture {
    let fx = Fixture::new(name);
    fx.write("Papers/Lease.docx", "old");
    drop(fx.import());
    fx
}

/// The matter opened again, as the worker opens it.
fn reopened(fx: &Fixture) -> Matter {
    let mut matter = fx.open(&fx.folder).unwrap();
    recover_backups(&mut matter);
    matter
}

#[test]
fn a_backup_whose_file_is_missing_is_put_back() {
    let fx = imported("backups-put-back");
    support::rename(fx.file("Papers/Lease.docx"), fx.file(BACKUP));
    drop(reopened(&fx));
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
    assert!(!fx.exists(BACKUP));
}

/// The open that reads (the search thread's, and every open on Mac and Linux) touches nothing.
#[test]
fn a_plain_open_leaves_the_backups_alone() {
    let fx = imported("backups-plain-open");
    support::rename(fx.file("Papers/Lease.docx"), fx.file(BACKUP));
    drop(fx.open(&fx.folder).unwrap());
    assert!(!fx.exists("Papers/Lease.docx"));
    assert_eq!(fx.read(BACKUP), b"old");
}

#[test]
fn a_stray_backup_the_store_holds_is_removed() {
    let fx = imported("backups-stray-held");
    fx.write(BACKUP, "old");
    fx.write("Papers/Lease.docx", "new");
    drop(reopened(&fx));
    assert!(!fx.exists(BACKUP));
    assert_eq!(fx.read("Papers/Lease.docx"), b"new");
}

/// A stray the store does not hold is kept; when the lawyer later deletes the file, the next
/// open does not bring it back.
#[test]
fn a_stray_backup_the_store_does_not_hold_is_kept_and_never_put_back() {
    let fx = imported("backups-stray-unknown");
    fx.write(BACKUP, "never versioned");
    drop(reopened(&fx));
    assert_eq!(fx.read(BACKUP), b"never versioned");
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");

    support::remove_file(fx.file("Papers/Lease.docx"));
    drop(reopened(&fx));
    assert!(!fx.exists("Papers/Lease.docx"));
    assert_eq!(fx.read(BACKUP), b"never versioned");
}

/// A file an agent wrote under a backup name, for a file the lawyer deleted, is not put back.
#[test]
fn a_backup_an_agent_wrote_is_not_put_back() {
    let fx = imported("backups-agent-wrote");
    support::remove_file(fx.file("Papers/Lease.docx"));
    fx.write(BACKUP, "planted");
    drop(reopened(&fx));
    assert!(!fx.exists("Papers/Lease.docx"));
    assert_eq!(fx.read(BACKUP), b"planted");
}

/// A backup whose content the store holds, with its file missing, is put back, never removed;
/// and the backup is never versioned.
#[test]
fn a_backup_is_never_removed_while_its_file_is_missing() {
    let fx = imported("backups-never-removed");
    fx.write(BACKUP, "old");
    support::remove_file(fx.file("Papers/Lease.docx"));
    let matter = reopened(&fx);
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
    let found = matter.papers(100).unwrap();
    assert!(!format!("{found:?}").contains("orig.tmp"), "{found:?}");
}

/// Two backups of one name: the one the store holds goes back, whatever the listing's order.
#[test]
fn of_two_backups_the_held_one_goes_back() {
    let fx = imported("backups-two");
    let other = "Papers/Lease.docx.clauseo-1.orig.tmp";
    support::remove_file(fx.file("Papers/Lease.docx"));
    fx.write(BACKUP, "planted");
    fx.write(other, "old");
    drop(reopened(&fx));
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
    assert_eq!(fx.read(BACKUP), b"planted");
    assert!(!fx.exists(other));
}

/// A name that is not UTF-8 gets its backup back.
#[cfg(target_os = "linux")]
#[test]
fn a_name_that_is_not_utf8_gets_its_backup_back() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt as _;
    let fx = imported("backups-not-utf8");
    let dir = fx.file("Papers");
    let target = dir.join(OsStr::from_bytes(b"Lease \xff.docx"));
    let backup = dir.join(OsStr::from_bytes(b"Lease \xff.docx.clauseo-0.orig.tmp"));
    support::write(&backup, "old");
    drop(reopened(&fx));
    assert_eq!(std::fs::read(&target).unwrap(), b"old");
    assert!(!backup.exists());
}

/// A 240-unit name, too long for `<name>.clauseo-<n>.orig.tmp`.
fn long() -> String {
    format!("Papers/{}.docx", "a".repeat(235))
}

/// [`long`]'s backup name: its FNV-1a hash, the same in every build.
const LONG_BACKUP: &str = "Papers/.clauseo-6f79de2aa1282bde-0.orig.tmp";

/// `Papers/<240 units>` "old" imported, the matter closed.
fn imported_long(name: &str) -> Fixture {
    let fx = Fixture::new(name);
    fx.write(&long(), "old");
    drop(fx.import());
    fx
}

/// A crash left a long-named file under its hashed backup name: matched through the cached
/// name, it is put back.
#[test]
fn a_hashed_backup_whose_file_is_missing_is_put_back() {
    let fx = imported_long("backups-hashed-put-back");
    support::rename(fx.file(&long()), fx.file(LONG_BACKUP));
    drop(reopened(&fx));
    assert_eq!(fx.read(&long()), b"old");
    assert!(!fx.exists(LONG_BACKUP));
}

/// A hashed backup whose hash is no cached name's is kept, even with content the store holds
/// and a missing long-named file beside it: never put back on a guess.
#[test]
fn a_hashed_backup_of_no_cached_name_is_kept() {
    let fx = imported_long("backups-hashed-unknown");
    let other = "Papers/.clauseo-0123456789abcdef-0.orig.tmp";
    support::rename(fx.file(&long()), fx.file(other));
    drop(reopened(&fx));
    assert!(!fx.exists(&long()));
    assert_eq!(fx.read(other), b"old");
}
