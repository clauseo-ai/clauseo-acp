//! The check after the walk: a kept key whose owner or DACL a held handle changed back is taken
//! over again once, and set-up stops when it changes back again.

use super::{ADMIN_KEY, Event, FakeTree, USER_KEY, under};
use crate::windows::setup::record_key::VENDOR_KEY;
use crate::windows::setup::vendor_key::{INHERITED_SDDL, VENDOR_SDDL, VendorFault, ensure};

#[test]
fn a_kept_key_changed_back_after_its_take_over_is_taken_over_again() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("A", ADMIN_KEY, false), ("B", ADMIN_KEY, false)],
    );
    // B's take-over comes after A is kept, as a planter's handle on A writes.
    tree.events = vec![(under("B"), Event::Undo(under("A"), USER_KEY.to_owned()))];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.retaken, [under("A")]);
    assert_eq!(tree.sddl_of(&under("A")), INHERITED_SDDL);
    assert_eq!(tree.sddl_of(&under("B")), INHERITED_SDDL);
}

#[test]
fn a_root_changed_back_is_taken_over_again() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("A", ADMIN_KEY, false)]);
    tree.events = vec![(
        under("A"),
        Event::Undo(VENDOR_KEY.to_owned(), USER_KEY.to_owned()),
    )];
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.retaken, [VENDOR_KEY.to_owned()]);
    assert_eq!(tree.sddl_of(VENDOR_KEY), VENDOR_SDDL);
}

#[test]
fn a_key_changed_back_again_after_its_second_take_over_refuses_set_up() {
    let mut tree = FakeTree::with(
        Some(VENDOR_SDDL),
        &[("A", ADMIN_KEY, false), ("B", ADMIN_KEY, false)],
    );
    tree.events = vec![(under("B"), Event::Undo(under("A"), USER_KEY.to_owned()))];
    tree.retake_undone = Some((under("A"), USER_KEY.to_owned()));
    let result = ensure(&mut tree, VENDOR_KEY);
    assert!(
        matches!(&result, Err(VendorFault::Secure(why)) if why.contains("changed back")),
        "{result:?}"
    );
}

#[test]
fn a_walk_nothing_changed_back_takes_nothing_over_again() {
    let mut tree = FakeTree::with(Some(VENDOR_SDDL), &[("A", ADMIN_KEY, false)]);
    let report = ensure(&mut tree, VENDOR_KEY).unwrap();
    assert_eq!(report.retaken, Vec::<String>::new());
}
