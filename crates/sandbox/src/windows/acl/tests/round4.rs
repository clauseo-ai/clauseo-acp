//! Round 4 of the S15 `PR3b` review on a real NTFS volume: forget's hand-back walk over a child
//! the lawyer can't open.

use super::*;

/// Round 4, from the second VM run: forget over a child the lawyer can't open at all failed
/// `Io(PermissionDenied)` naming nothing, since only a strict walk mapped a refused open to
/// `ChildShut`. The hand-back walk now refuses it as `ChildShut` too, naming it, so forget
/// sets it aside.
#[test]
fn a_child_the_lawyer_cant_open_refuses_the_handback_naming_it() {
    drop_privileges();
    let place = Place::new("acl-reclaim-shut");
    let shut = place.dir.file("matter/shut.txt", "x");
    protect_with(&shut, "D:P(A;;FA;;;SY)(A;;0x0;;;OW)").unwrap();
    let mut acl = NtfsAcl::new().unwrap();
    let me = current_user_sid().unwrap();
    let walked = acl.reclaim(&place.at("matter"), &[me]);
    assert_eq!(walked, Err(ApplyFailure::ChildShut));
    let at = acl.refused_at().unwrap();
    assert!(at.ends_with("shut.txt"), "{at:?}");
}
