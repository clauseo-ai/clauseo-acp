//! The staging folder's descriptor on a real NTFS volume, made and read back, re-secured when
//! it is there, refused when a junction stands in for it; and the staged file's write, read
//! back, rename and delete. The tests' owner is this user, since only SYSTEM (or a holder of
//! `SeRestorePrivilege`) may make SYSTEM the owner; the production SDDL's own read-back match is
//! tested with the engine.

use super::*;
use crate::test_support::TempDir;
use crate::windows::record::current_user_sid;
use crate::windows::refresh::source::read_capped;
use crate::windows::test_junction::junction;

/// This user alone, full control, inherited; protected: `STAGING_SDDL` with this user for
/// SYSTEM.
fn own_sddl() -> String {
    let me = current_user_sid().unwrap();
    format!("O:{me}D:P(A;OICI;FA;;;{me})")
}

#[test]
fn the_staging_folder_is_made_with_its_dacl_and_reads_back() {
    let dir = TempDir::new("refresh-staging-new");
    let staging = dir.path().join("staging");
    secure_dir(&staging, &own_sddl()).unwrap();
    let found = sddl_of(&open_dir(&staging).unwrap()).unwrap();
    assert!(
        crate::windows::refresh::sddl_matches(&found, &own_sddl()),
        "{found}"
    );
}

#[test]
fn a_staging_folder_already_there_is_secured_again() {
    let dir = TempDir::new("refresh-staging-again");
    let staging = dir.dir("staging");
    // Inherited entries from the temp folder, unprotected: replaced by the protected DACL.
    secure_dir(&staging, &own_sddl()).unwrap();
    let found = sddl_of(&open_dir(&staging).unwrap()).unwrap();
    assert!(found.contains("D:P"), "{found}");
    assert!(
        crate::windows::refresh::sddl_matches(&found, &own_sddl()),
        "{found}"
    );
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
