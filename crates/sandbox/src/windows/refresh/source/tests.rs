//! The user folder's reads on a real NTFS volume: a junction at the exe's name, a second hard
//! link, an oversize file and a missing one are each refused by name; a plain file is read.

use super::*;
use crate::test_support::TempDir;
use crate::windows::test_junction::junction;

#[test]
fn a_plain_file_is_read_whole() {
    let dir = TempDir::new("refresh-source-plain");
    let file = dir.file("clauseo-sandbox-setup.exe", "MZ the setup exe");
    assert_eq!(read_capped(&file, 1024).unwrap(), b"MZ the setup exe");
    // At the cap exactly, still read.
    assert_eq!(read_capped(&file, 16).unwrap().len(), 16);
}

#[test]
fn a_junction_at_the_name_is_refused_not_followed() {
    let dir = TempDir::new("refresh-source-junction");
    let target = dir.dir("elsewhere");
    let link = dir.path().join("clauseo-sandbox-setup.exe");
    junction(&link, &target).unwrap();
    // IO_REPARSE_TAG_MOUNT_POINT, the junction's own tag: opened as itself.
    assert_eq!(
        read_capped(&link, 1024),
        Err(SourceError::ReparsePoint(0xA000_0003))
    );
}

#[test]
fn a_second_hard_link_is_refused() {
    let dir = TempDir::new("refresh-source-link");
    let file = dir.file("clauseo-sandbox-setup.exe", "MZ");
    let _other = dir.hard_link("elsewhere.exe", &file);
    assert_eq!(read_capped(&file, 1024), Err(SourceError::Linked(2)));
}

#[test]
fn an_oversize_or_missing_file_is_refused() {
    let dir = TempDir::new("refresh-source-size");
    let file = dir.file("clauseo-sandbox-setup.exe.sig", &"A".repeat(4097));
    assert_eq!(read_capped(&file, 4096), Err(SourceError::TooLarge(4096)));
    let missing = dir.path().join("clauseo-sandbox-setup.exe");
    assert_eq!(read_capped(&missing, 4096), Err(SourceError::Missing));
    let missing_dir = dir.path().join("gone").join("clauseo-sandbox-setup.exe");
    assert_eq!(read_capped(&missing_dir, 4096), Err(SourceError::Missing));
}

#[test]
fn a_folder_at_the_name_is_refused() {
    let dir = TempDir::new("refresh-source-folder");
    let folder = dir.dir("clauseo-sandbox-setup.exe");
    assert!(read_capped(&folder, 4096).is_err());
}
