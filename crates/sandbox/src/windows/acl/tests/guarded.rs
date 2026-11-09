//! #808's M1 on a real NTFS volume: `CPython`'s `tempfile.mkdtemp` folder,
//! `D:P(A;OICI;FA;;;OW)`, in a guarded matter loses its `OWNER RIGHTS` allow at the strict walk,
//! so the guard's execute deny reaches the files below it.

use super::*;
use crate::windows::shared_writes::OWNER_RIGHTS_SID;
use crate::windows::test_acl::entries;

#[test]
fn pythons_temp_folder_in_a_guarded_matter_loses_its_owner_allow() {
    let place = Place::new("acl-guard-tempfile");
    let tool = place.dir.file("matter/tmpabc/tool.exe", "MZ");
    let made = place.at("matter/tmpabc");
    own(&made);
    protect_with(&made, "D:P(A;OICI;FA;;;OW)").unwrap();
    let matter = place.at("matter");
    let me = current_user_sid().unwrap();
    let guard = [
        grant(
            &matter,
            me.as_str(),
            Rights::DenyExecute,
            Inherit::FilesBelow,
        ),
        grant(
            &matter,
            OWNER_RIGHTS_SID,
            Rights::OwnerModify,
            Inherit::FilesBelow,
        ),
    ];
    let mut acl = NtfsAcl::new().unwrap();
    for g in &guard {
        acl.add(g).unwrap();
    }
    let strict = Strict {
        watched: vec![Sid::parse(OTHER_POOL_SIDS[0]).unwrap()],
        pool: vec![me.clone()],
        assigned: Vec::new(),
        guarded: vec![me.clone(), Sid::parse(OWNER_RIGHTS_SID).unwrap()],
    };
    let walked = acl.propagate(&matter, Reach::Tree, Some(&strict)).unwrap();
    assert!(walked.repaired >= 1, "{walked:?}");
    assert!(!protected(&made));
    assert!(
        !has(&made, OWNER_RIGHTS_SID, false),
        "the explicit allow went"
    );
    assert!(
        has(&tool, me.as_str(), true),
        "the execute deny reaches the file"
    );
    let explicit_ow = entries(&tool)
        .into_iter()
        .any(|(_, flags, _, sid)| flags & INHERITED == 0 && sid.as_str() == OWNER_RIGHTS_SID);
    assert!(!explicit_ow);
}
