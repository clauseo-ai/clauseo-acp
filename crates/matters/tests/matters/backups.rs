//! The backups a crashed rewrite left (`<name>.clauseo-<n>.orig.tmp`), as the matter opens again:
//! put back where the file is missing, removed where the file is there and the store holds the
//! backup's content, kept otherwise.

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

#[test]
fn a_backup_whose_file_is_missing_is_put_back() {
    let fx = imported("backups-put-back");
    support::rename(fx.file("Papers/Lease.docx"), fx.file(BACKUP));
    let matter = fx.open(&fx.folder).unwrap();
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
    assert!(!fx.exists(BACKUP));
    drop(matter);
}

#[test]
fn a_stray_backup_the_store_holds_is_removed() {
    let fx = imported("backups-stray-held");
    fx.write(BACKUP, "old");
    fx.write("Papers/Lease.docx", "new");
    drop(fx.open(&fx.folder).unwrap());
    assert!(!fx.exists(BACKUP));
    assert_eq!(fx.read("Papers/Lease.docx"), b"new");
}

#[test]
fn a_stray_backup_the_store_does_not_hold_is_kept() {
    let fx = imported("backups-stray-unknown");
    fx.write(BACKUP, "never versioned");
    drop(fx.open(&fx.folder).unwrap());
    assert_eq!(fx.read(BACKUP), b"never versioned");
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
}

/// A backup whose content the store holds, with its file missing, is put back, never removed;
/// and the backup is never versioned.
#[test]
fn a_backup_is_never_removed_while_its_file_is_missing() {
    let fx = imported("backups-never-removed");
    fx.write(BACKUP, "old");
    support::remove_file(fx.file("Papers/Lease.docx"));
    let matter = fx.open(&fx.folder).unwrap();
    assert_eq!(fx.read("Papers/Lease.docx"), b"old");
    let found = matter.papers(100).unwrap();
    assert!(!format!("{found:?}").contains("orig.tmp"), "{found:?}");
}
