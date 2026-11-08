//! Round 2 of the S15 `PR3b` review on a real NTFS volume: files with more than one link (N2), a
//! repair the handle has no right for (N3), and the credential link's write (N8).

use super::*;

/// N2: a file the agent owns (this process's user stands in for it), open to `Everyone` by its
/// own entry and with a second hard link, is repaired by a strict walk and handed back by
/// `reclaim`: its DACL belongs to the file, not to one of its names.
#[test]
fn a_file_the_agent_owns_with_a_second_link_is_repaired_and_handed_back() {
    let place = Place::new("acl-linked-repair");
    let doc = place.dir.file("matter/doc.txt", "the agent's");
    place.dir.hard_link("matter/sub/again.txt", &doc);
    let doc = place.at("matter/doc.txt");
    own(&doc);
    let open = "D:P(A;;FA;;;OW)(A;;FR;;;WD)";
    protect_with(&doc, open).unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    let walked = as_agent(&mut acl, &place.at("matter")).unwrap();
    assert!(walked.repaired >= 1, "{walked:?}");
    assert!(!protected(&doc));
    assert!(!has(&doc, "S-1-1-0", false));
    assert!(has(&doc, OTHER_POOL_SIDS[0], true));

    protect_with(&doc, open).unwrap();
    let me = current_user_sid().unwrap();
    let walked = acl.reclaim(&place.at("matter"), &[me]).unwrap();
    assert!(walked.repaired >= 1, "{walked:?}");
    assert!(!protected(&doc));
    assert!(!has(&doc, "S-1-1-0", false));
}

/// N3: a file the agent owns whose own DACL takes the owner's implicit `WRITE_DAC` away (an
/// `OWNER RIGHTS` entry for `READ_CONTROL` only) opens read only, so the repair can't write it:
/// the strict walk refuses it as `ChildShut`, naming it, for the launch to set it aside.
#[test]
fn an_agent_file_that_leaves_no_write_dac_refuses_as_child_shut_naming_it() {
    drop_privileges();
    let place = Place::new("acl-no-write-dac");
    place.dir.file("matter/locked.txt", "the agent's");
    let locked = place.at("matter/locked.txt");
    own(&locked);
    protect_with(&locked, "D:P(A;;RC;;;OW)(A;;FR;;;WD)").unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    let walked = as_agent(&mut acl, &place.at("matter"));
    assert_eq!(walked, Err(ApplyFailure::ChildShut));
    let at = acl.refused_at().unwrap();
    assert!(at.ends_with("locked.txt"), "{at:?}");
}

/// N8: the credential link's write compares the file ID on the handle it writes through. A
/// link that is a hard link to the expected file is written; a link that is a hard link to
/// another file (as one swapped since the check would be) is refused as `Changed`, and that
/// file gets no entry.
#[test]
fn a_credential_link_is_written_only_when_it_is_the_expected_file() {
    let place = Place::new("acl-credential-link");
    let creds = place.dir.file("home/creds.json", "{}");
    let other = place.dir.file("home/other.json", "{}");
    place.dir.hard_link("state/creds.json", &creds);
    place.dir.hard_link("swapped/creds.json", &other);
    let creds = place.at("home/creds.json");
    let link = |rel: &str| {
        let mut write = grant(
            &place.at(rel),
            USER_SID,
            Rights::ReadWriteData,
            Inherit::ThisOnly,
        );
        write.kind = PathKind::CredentialLink;
        write
    };
    let mut acl = NtfsAcl::new().unwrap();
    let same = acl.add_expecting(&link("state/creds.json"), &creds);
    assert_eq!(same, Ok(Added::Written));
    assert!(has(&creds, USER_SID, false));

    let swapped = acl.add_expecting(&link("swapped/creds.json"), &creds);
    assert_eq!(swapped, Err(ApplyFailure::Changed));
    assert!(!has(&place.at("home/other.json"), USER_SID, false));
}
