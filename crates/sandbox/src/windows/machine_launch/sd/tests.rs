//! The descriptor's reading, edits and evaluation: Windows 11's value in both layouts, exact
//! bytes back, the deny's placement, COM's two forms, and every malformed shape refused.

use super::super::test_sd::{
    A, D, GROUP, OTHER_GROUP, ace, acl, descriptor, sid, windows_11, windows_11_aces,
};
use super::{DENY_MASK, LaunchSd, SdError};

#[test]
fn windows_11s_value_reads_and_writes_back_byte_for_byte_in_either_layout() {
    for dacl_first in [false, true] {
        let bytes = descriptor(&windows_11_aces(), dacl_first);
        let parsed = LaunchSd::parse(&bytes).unwrap();
        assert_eq!(parsed.to_bytes(), bytes, "dacl_first {dacl_first}");
        assert_eq!(parsed.aces(), windows_11_aces().as_slice());
    }
}

#[test]
fn the_deny_goes_first_and_every_entry_stays_as_it_was() {
    let parsed = LaunchSd::parse(&windows_11()).unwrap();
    assert!(!parsed.denies_first(&sid(GROUP)));
    let merged = parsed.with_deny(&sid(GROUP)).unwrap();
    assert_eq!(merged.aces().first(), Some(&ace(D, 0, DENY_MASK, GROUP)));
    assert_eq!(merged.aces().get(1..), Some(windows_11_aces().as_slice()));
    assert!(merged.denies_first(&sid(GROUP)));
    // Once is enough.
    assert_eq!(merged.with_deny(&sid(GROUP)), None);
    // It reads back as itself, and taking it off gives the bytes read.
    let bytes = merged.to_bytes();
    assert_eq!(LaunchSd::parse(&bytes).unwrap(), merged);
    let (kept, dropped) = merged.without_denies(&sid(GROUP));
    assert_eq!((kept.to_bytes(), dropped), (windows_11(), 1));
}

#[test]
fn a_weak_misplaced_inherit_only_or_other_groups_deny_gets_ours_in_front() {
    let cases = [
        // Without the remote rights.
        vec![ace(D, 0, 0xB, GROUP), ace(A, 0, 0x1F, "S-1-5-32-544")],
        // After an allow.
        vec![ace(A, 0, 0x1F, "S-1-5-32-544"), ace(D, 0, 0x1F, GROUP)],
        // Inherit-only, so it doesn't apply.
        vec![ace(D, 0x8, 0x1F, GROUP), ace(A, 0, 0x1F, "S-1-5-32-544")],
        // Another group's.
        vec![
            ace(D, 0, 0x1F, OTHER_GROUP),
            ace(A, 0, 0x1F, "S-1-5-32-544"),
        ],
    ];
    for aces in cases {
        let parsed = LaunchSd::parse(&descriptor(&aces, false)).unwrap();
        let merged = parsed.with_deny(&sid(GROUP)).unwrap();
        assert_eq!(merged.aces().first(), Some(&ace(D, 0, DENY_MASK, GROUP)));
        assert_eq!(merged.aces().get(1..), Some(aces.as_slice()));
    }
    // An earlier full deny of ours behind another group's deny counts: denies come first.
    let aces = [
        ace(D, 0, 0x1F, OTHER_GROUP),
        ace(D, 0, 0x1F, GROUP),
        ace(A, 0, 0x1F, "S-1-5-32-544"),
    ];
    let parsed = LaunchSd::parse(&descriptor(&aces, false)).unwrap();
    assert_eq!(parsed.with_deny(&sid(GROUP)), None);
    // The removal takes our exact deny and nothing else.
    let (kept, dropped) = parsed.without_denies(&sid(GROUP));
    assert_eq!(dropped, 1);
    assert_eq!(kept.aces(), [aces[0].clone(), aces[2].clone()].as_slice());
}

#[test]
fn the_removal_keeps_a_near_miss_deny_for_the_group() {
    let near = [
        // Another mask.
        ace(D, 0, 0xB, GROUP),
        // Flags (container inherit).
        ace(D, 0x2, DENY_MASK, GROUP),
        // More rights than ours.
        ace(D, 0, 0x7FF, GROUP),
    ];
    let mut aces = vec![ace(D, 0, DENY_MASK, GROUP)];
    aces.extend(near.iter().cloned());
    aces.push(ace(A, 0, 0x1F, "S-1-5-32-544"));
    let parsed = LaunchSd::parse(&descriptor(&aces, false)).unwrap();
    let (kept, dropped) = parsed.without_denies(&sid(GROUP));
    assert_eq!(dropped, 1);
    assert_eq!(kept.aces().get(..3), Some(near.as_slice()));
    assert_eq!(kept.aces().len(), 4);
}

#[test]
fn windows_default_is_the_documented_one() {
    let default = LaunchSd::windows_default();
    assert_eq!(
        default.aces(),
        [ace(A, 0, 0x1F, "S-1-5-32-544"), ace(A, 0, 0xB, "S-1-1-0")].as_slice()
    );
    assert_eq!(LaunchSd::parse(&default.to_bytes()).unwrap(), default);
    assert_eq!(default.to_bytes(), descriptor(default.aces(), false));
}

#[test]
fn granted_follows_access_checks_order() {
    let aces = [
        ace(D, 0, 0x1F, GROUP),
        ace(A, 0, 0x1F, "S-1-5-32-544"),
        ace(A, 0, 0xB, "S-1-1-0"),
        ace(A, 0x8, 0x1F, "S-1-1-0"),
    ];
    let parsed = LaunchSd::parse(&descriptor(&aces, false)).unwrap();
    assert_eq!(parsed.granted(&[sid("S-1-1-0")]), 0xB);
    assert_eq!(parsed.granted(&[sid("S-1-1-0"), sid("S-1-5-32-544")]), 0x1F);
    assert_eq!(parsed.granted(&[sid("S-1-1-0"), sid(GROUP)]), 0);
    // An allow before the deny wins for the rights it names.
    let aces = [ace(A, 0, 0xB, "S-1-1-0"), ace(D, 0, 0x1F, GROUP)];
    let parsed = LaunchSd::parse(&descriptor(&aces, false)).unwrap();
    assert_eq!(parsed.granted(&[sid("S-1-1-0"), sid(GROUP)]), 0xB);
}

#[test]
fn every_malformed_shape_is_refused() {
    let good = windows_11();
    let mut cases: Vec<(Vec<u8>, &str)> = vec![
        (Vec::new(), "shorter than its header"),
        (good[..19].to_vec(), "shorter than its header"),
    ];
    let mut edit = |at: usize, value: u8, why: &'static str| {
        let mut bytes = good.clone();
        bytes[at] = value;
        cases.push((bytes, why));
    };
    edit(0, 2, "not revision 1");
    edit(3, 0x00, "not self-relative");
    edit(2, 0x00, "no DACL, or a NULL one");
    edit(4, 0, "no owner or no group");
    edit(4, 8, "an offset inside the header");
    edit(5, 0xFF, "a SID past the end");
    // The DACL at 52 (20 + 16 + 16): its revision, then its size.
    edit(52, 3, "an ACL of an unknown revision");
    edit(54, 6, "an ACL of an odd size");
    edit(55, 0x7F, "an ACL past the end");
    // The first entry's size.
    edit(62, 6, "an entry of an odd size");
    edit(62, 0xFC, "an entry past its ACL");
    // Its SID's count, beyond what it holds.
    edit(69, 15, "an entry whose SID isn't whole");
    // A NULL DACL: the present bit with no offset.
    let mut null = good.clone();
    null[16..20].copy_from_slice(&[0; 4]);
    cases.push((null, "no DACL, or a NULL one"));
    // COM's rules.
    let no_execute = descriptor(&[ace(A, 0, 0x1E, "S-1-1-0")], false);
    cases.push((no_execute, "an entry without COM_RIGHTS_EXECUTE"));
    let old_form = descriptor(&[ace(A, 0, 0x1, "S-1-1-0")], false);
    cases.push((
        old_form,
        "an entry in COM's old form, which a deny would mix with",
    ));
    for (bytes, why) in cases {
        assert_eq!(LaunchSd::parse(&bytes), Err(SdError(why)), "{bytes:?}");
    }
    // An empty DACL is a DACL: it grants no one, and the deny can still go first.
    let empty = LaunchSd::parse(&descriptor(&[], false)).unwrap();
    assert_eq!(empty.with_deny(&sid(GROUP)).unwrap().aces().len(), 1);
    assert_eq!(acl(&[]).len(), 8);
}
