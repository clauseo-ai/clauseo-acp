//! Round 5 of the S15 `PR3b` review on a real NTFS volume: an entry taken back from the file
//! it recorded, opened by its file ID.

use super::*;
use crate::test_support::remove;
use crate::windows::ledger::RecordedFile;
use crate::windows::write::rename_new;

/// Round 5, issue 1: the credentials file's entry was removed through its recorded path, so a
/// renamed original kept it and a deleted one was judged by whatever was at the path. Opened by
/// its file ID (`OpenFileById`) on its volume, the renamed original loses the entry, the new
/// file at the path keeps its own, and once the original is deleted nothing is found and the
/// removal is `Ok`.
#[test]
fn an_entry_recorded_by_file_id_comes_off_the_original_and_not_the_new_file_at_its_path() {
    let place = Place::new("acl-by-id");
    let path = dunce::canonicalize(place.dir.file("creds/auth.json", "{}")).unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    let modify = grant(&path, USER_SID, Rights::Modify, Inherit::ThisOnly);
    acl.add(&modify).unwrap();
    let file = RecordedFile::new(path.clone(), acl.file_id(&path).unwrap().unwrap());
    let moved = place.at("creds/moved.json");
    rename_new(&path, &moved).unwrap();
    place.dir.file("creds/auth.json", "new");
    acl.add(&modify).unwrap();
    assert!(acl.present_recorded(&file, &modify).unwrap());

    acl.remove_recorded(&file, &modify, None).unwrap();
    assert!(!has(&moved, USER_SID, false), "the original lost its entry");
    assert!(
        has(&path, USER_SID, false),
        "the new file at the path kept its own"
    );
    assert!(!acl.present_recorded(&file, &modify).unwrap());

    remove(&moved);
    assert!(!acl.present_recorded(&file, &modify).unwrap());
    assert_eq!(acl.remove_recorded(&file, &modify, None), Ok(()));
}
