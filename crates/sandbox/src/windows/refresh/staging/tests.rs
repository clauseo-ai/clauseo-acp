//! The staging folder's descriptor on a real NTFS volume, made and read back, re-secured when
//! it is there, refused when a junction stands in for it; and the staged file's write, read
//! back, rename and delete; and the admin folder's hold, which keeps out a handle that would fail
//! a rename into it. The tests' owner is this user, since only SYSTEM (or a holder of
//! `SeRestorePrivilege`) may make SYSTEM the owner; the production SDDL's own read-back match is
//! tested with the engine. A read-back is compared with what `as_read_back` makes of the SDDL,
//! which writes this user's SID as Windows does (`LA` when CI runs as the built-in
//! Administrator).

use super::*;
use crate::test_support::TempDir;
use crate::windows::record::current_user_sid;
use crate::windows::refresh::source::read_capped;
use crate::windows::refresh::{STAGING_SDDL, sddl_matches};
use crate::windows::test_junction::junction;

/// This user alone, full control, inherited; protected: `STAGING_SDDL` with this user for
/// SYSTEM.
fn own_sddl() -> String {
    let me = current_user_sid().unwrap();
    format!("O:{me}D:P(A;OICI;FA;;;{me})")
}

/// Whether `found`, read back, is `expected` in the form Windows writes it.
fn reads_back_as(found: &str, expected: &str) -> bool {
    sddl_matches(found, &as_read_back(expected).unwrap())
}

#[test]
fn an_sddl_is_written_back_as_windows_writes_it() {
    // A well-known SID by its number comes back as its alias; an alias stays as it is.
    assert_eq!(
        as_read_back("O:S-1-5-18D:P(A;OICI;FA;;;S-1-5-18)(A;OICI;FA;;;S-1-5-32-544)").unwrap(),
        STAGING_SDDL
    );
    assert_eq!(as_read_back(STAGING_SDDL).unwrap(), STAGING_SDDL);
    assert_eq!(
        as_read_back("O:BAD:P(A;;FA;;;S-1-5-32-544)").unwrap(),
        "O:BAD:P(A;;FA;;;BA)"
    );
    // Not SDDL: an error, never a match.
    assert!(as_read_back("O:SYD:P(A;;FA;;;not-a-sid)").is_err());
}

#[test]
fn a_descriptor_naming_this_user_matches_its_read_back() {
    let dir = TempDir::new("refresh-staging-me");
    let staging = dir.path().join("staging");
    secure_dir(&staging, &own_sddl()).unwrap();
    let found = sddl_of(&open_dir(&staging, SECURITY_ACCESS, FILE_SHARE_MODE(0)).unwrap()).unwrap();
    assert!(reads_back_as(&found, &own_sddl()), "{found}");
    // Its read-back is this user's, not SYSTEM's.
    assert!(!reads_back_as(&found, STAGING_SDDL), "{found}");
}

#[test]
fn the_staging_folder_is_made_with_its_dacl_and_reads_back() {
    let dir = TempDir::new("refresh-staging-new");
    let staging = dir.path().join("staging");
    secure_dir(&staging, &own_sddl()).unwrap();
    let found = sddl_of(&open_dir(&staging, SECURITY_ACCESS, FILE_SHARE_MODE(0)).unwrap()).unwrap();
    assert!(reads_back_as(&found, &own_sddl()), "{found}");
}

#[test]
fn a_staging_folder_already_there_is_secured_again() {
    let dir = TempDir::new("refresh-staging-again");
    let staging = dir.dir("staging");
    // Inherited entries from the temp folder, unprotected: replaced by the protected DACL.
    secure_dir(&staging, &own_sddl()).unwrap();
    let found = sddl_of(&open_dir(&staging, SECURITY_ACCESS, FILE_SHARE_MODE(0)).unwrap()).unwrap();
    assert!(found.contains("D:P"), "{found}");
    assert!(reads_back_as(&found, &own_sddl()), "{found}");
}

#[test]
fn a_junction_for_the_staging_folder_is_refused() {
    let dir = TempDir::new("refresh-staging-junction");
    let target = dir.dir("elsewhere");
    let staging = dir.path().join("staging");
    junction(&staging, &target).unwrap();
    assert!(secure_dir(&staging, &own_sddl()).is_err());
}

#[test]
fn the_staged_file_is_written_read_back_moved_and_deleted() {
    let dir = TempDir::new("refresh-staging-file");
    let staging = dir.path().join("staging");
    secure_dir(&staging, &own_sddl()).unwrap();
    let staged = staging.join("clauseo-sandbox-setup.exe.new");
    let bytes: Vec<u8> = (0..3_000_000u32).map(|n| (n % 253) as u8).collect();
    write_new(&staged, &bytes, &own_sddl()).unwrap();
    assert_eq!(read_capped(&staged, 16 * 1024 * 1024).unwrap(), bytes);
    // Written again over a leftover: the old one is deleted first.
    write_new(&staged, b"MZ again", &own_sddl()).unwrap();
    assert_eq!(read_capped(&staged, 1024).unwrap(), b"MZ again");
    let placed = dir.path().join("clauseo-sandbox-setup.exe");
    rename(&staged, &placed).unwrap();
    assert!(rename(&staged, &placed).is_err(), "nothing left to move");
    delete(&placed).unwrap();
    delete(&placed).unwrap();
    remove_dir(&staging).unwrap();
    remove_dir(&staging).unwrap();
}

#[test]
fn a_held_folder_keeps_out_a_read_only_share_and_still_takes_a_rename() {
    let dir = TempDir::new("refresh-hold");
    let admin = dir.dir("Sandbox");
    let from = dir.path().join("new.exe");
    write_new(&from, b"new", &own_sddl()).unwrap();
    let read_data = 0x0001; // FILE_READ_DATA (FILE_LIST_DIRECTORY)
    let flags = FILE_FLAG_BACKUP_SEMANTICS.0;
    // Someone holding the folder with sharing read only stops the hold before anything moves.
    let blocker = open(&admin, read_data, FILE_SHARE_READ, flags).unwrap();
    assert!(hold_dir(&admin).is_err());
    drop(blocker);
    // Held, no such handle can be opened, and a rename into the folder still works.
    let held = hold_dir(&admin).unwrap();
    assert!(open(&admin, read_data, FILE_SHARE_READ, flags).is_err());
    rename(&from, &admin.join("placed.exe")).unwrap();
    drop(held);
    assert!(admin.join("placed.exe").is_file());
}
