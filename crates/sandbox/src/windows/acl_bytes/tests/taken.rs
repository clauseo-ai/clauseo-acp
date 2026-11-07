//! What `--reclaim` took, cleaned: only the explicit allows of pool users other than the
//! matter's own go (in a protected DACL, one marked inherited too: that is forged); a broad
//! group's allow, the matter's own pool user's, inherited entries of an unprotected DACL,
//! denies and the DACL's protection stay. Nothing else the lawyer owns is touched.

use super::*;

fn others() -> [Vec<u8>; 1] {
    [sid_bytes(&sid(HB_OTHER))]
}

fn me() -> Vec<u8> {
    sid_bytes(&sid(HB_ME))
}

/// `read_back`'s descriptor with `control` set too.
fn with_control(owner: &str, aces: &[&[u8]], control: u16) -> Vec<u8> {
    let dacl = render(2, aces);
    let built = build_descriptor(control, Some(&dacl), None);
    rebuild(&built, Some(&sid_bytes(&sid(owner))), None, 0).unwrap()
}

#[test]
fn only_another_pool_users_explicit_allow_goes_and_protection_stays() {
    let everyone = spec("S-1-1-0", Rights::ReadAndExecute).bytes();
    let users = spec("S-1-5-32-545", Rights::Modify).bytes();
    let own = spec(HB_OWN, Rights::Modify).bytes();
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let inherited = inherited(HB_OTHER, Rights::Modify);
    let deny = ace(DENIED, 0, 0x001f_01ff, HB_OTHER);
    let all: [&[u8]; 6] = [&deny, &everyone, &users, &own, &other, &inherited];
    let before = with_control(HB_ME, &all, DACL_PROTECTED);

    let after = stripped(&before, &others(), &me()).unwrap().unwrap();

    let read = Descriptor::parse(&after).unwrap();
    // In a protected DACL the "inherited" allow is forged, so it goes too.
    let want: [&[u8]; 4] = [&deny, &everyone, &users, &own];
    assert_eq!(read.dacl, Some(render(2, &want).as_slice()));
    assert_eq!(
        read.control & DACL_PROTECTED,
        DACL_PROTECTED,
        "still protected"
    );
}

#[test]
fn nothing_to_strip_or_another_owner_is_left_alone() {
    let everyone = spec("S-1-1-0", Rights::Modify).bytes();
    let own = spec(HB_OWN, Rights::Modify).bytes();
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let none = |aces: &[&[u8]], owner: &str| {
        stripped(&read_back(owner, aces), &others(), &me())
            .unwrap()
            .is_none()
    };
    assert!(
        none(&[&everyone, &own], HB_ME),
        "no other pool user's allow"
    );
    assert!(
        none(&[&inherited(HB_OTHER, Rights::Modify)], HB_ME),
        "inherited"
    );
    assert!(none(&[&other], HB_OTHER), "a pool user's: main's hand-back");
    assert!(
        stripped(&read_back(HB_ME, &[&other]), &others(), &[])
            .unwrap()
            .is_none()
    );
}

/// An item the agent shut with only pool users' entries is left bare once stripped, and is
/// unprotected; one with anyone else's explicit entry left keeps its protection.
#[test]
fn a_shut_item_of_pool_entries_alone_is_left_bare_and_a_colleagues_isnt() {
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let shut = with_control(HB_ME, &[&other], DACL_PROTECTED);
    let after = stripped(&shut, &others(), &me()).unwrap().unwrap();
    assert!(bare(&after).unwrap());

    let colleague = spec("S-1-5-21-9-9-9-1500", Rights::ReadAndExecute).bytes();
    let shared = with_control(HB_ME, &[&other, &colleague], DACL_PROTECTED);
    let after = stripped(&shared, &others(), &me()).unwrap().unwrap();
    assert!(
        !bare(&after).unwrap(),
        "a colleague's entry keeps the protection"
    );

    let open = with_control(HB_ME, &[&other], 0);
    let after = stripped(&open, &others(), &me()).unwrap().unwrap();
    assert!(!bare(&after).unwrap(), "not protected: nothing to undo");
}

/// An agent forges an "inherited" allow in a protected DACL (`D:P(A;ID;FA;;;<pool user>)`):
/// nothing is inherited through a protected DACL, so it goes as an explicit one would, and the
/// item is left bare. In an unprotected DACL a real inherited entry stays.
#[test]
fn a_forged_inherited_allow_in_a_protected_dacl_goes() {
    let forged = inherited(HB_OTHER, Rights::Modify);
    let shut = with_control(HB_ME, &[&forged], DACL_PROTECTED);
    let after = stripped(&shut, &others(), &me()).unwrap().unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(empty_acl().as_slice()));
    assert!(bare(&after).unwrap());

    let real = with_control(HB_ME, &[&forged], DACL_AUTO_INHERITED);
    assert!(stripped(&real, &others(), &me()).unwrap().is_none());
}

/// The order, pinned (the orchestrator's request): the strip comes first and the inherited
/// grants are worked out from the stripped item, so no moment holds a pool user's allow and the
/// matter's grants at once; and it all goes out as one write, the final DACL. The fake
/// `inherit` records what it is given, as the walk's `CreatePrivateObjectSecurityEx` would be.
#[test]
fn a_bare_item_inherits_from_its_stripped_self_in_one_write() {
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let forged = inherited(HB_OTHER, Rights::Modify);
    let shut = with_control(HB_ME, &[&other, &forged], DACL_PROTECTED);
    let given = inherited(HB_OWN, Rights::Modify);
    let fenced = ace(DENIED, INHERITED, 0x001f_01ff, HB_OTHER);
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut fake = |base: &[u8]| {
        seen.push(base.to_vec());
        Ok(with_control(HB_ME, &[&fenced, &given], DACL_AUTO_INHERITED))
    };

    let write = taken_write(&shut, &others(), &me(), &mut fake).unwrap();

    assert_eq!(seen.len(), 1, "worked out once");
    let base = Descriptor::parse(&seen[0]).unwrap();
    assert_eq!(
        base.dacl,
        Some(empty_acl().as_slice()),
        "from the stripped item"
    );
    assert_eq!(base.control & DACL_PROTECTED, 0, "protected no more");
    let Some(TakenWrite::Inheriting(final_dacl)) = write else {
        panic!("expected one inheriting write, got {write:?}");
    };
    let read = Descriptor::parse(&final_dacl).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&fenced, &given]).as_slice()));
    assert_eq!(read.control & DACL_PROTECTED, 0);
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);
}

/// One with anyone else's entry left is stripped and keeps its protection: nothing is worked
/// out. One with nothing to strip gets no write.
#[test]
fn a_stripped_item_with_another_entry_left_inherits_nothing() {
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let colleague = spec("S-1-5-21-9-9-9-1500", Rights::ReadAndExecute).bytes();
    let mut calls = 0;
    let mut fake = |_: &[u8]| {
        calls += 1;
        Ok(Vec::new())
    };
    let shared = with_control(HB_ME, &[&other, &colleague], DACL_PROTECTED);
    let write = taken_write(&shared, &others(), &me(), &mut fake).unwrap();
    let Some(TakenWrite::Stripped(new)) = write else {
        panic!("expected a strip, got {write:?}");
    };
    let read = Descriptor::parse(&new).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&colleague]).as_slice()));
    assert_eq!(read.control & DACL_PROTECTED, DACL_PROTECTED);

    let clean = with_control(HB_ME, &[&colleague], DACL_PROTECTED);
    assert_eq!(
        taken_write(&clean, &others(), &me(), &mut fake).unwrap(),
        None
    );
    assert_eq!(calls, 0);
}

/// What was below the agent's protected folder: its entries are inherited copies of that DACL.
/// Once the folder inherits again, the item's inherited entries are worked out from its folder
/// too, from its explicit entries alone, so the copy naming the other pool user goes in the
/// same one write.
#[test]
fn an_item_left_with_an_inherited_copy_of_the_agents_allow_inherits_again() {
    let copy = inherited(HB_OTHER, Rights::Modify);
    let lawyers = spec(HB_ME, Rights::Modify).bytes();
    let below = with_control(HB_ME, &[&lawyers, &copy], DACL_AUTO_INHERITED);
    let given = inherited(HB_OWN, Rights::Modify);
    let mut seen: Vec<Vec<u8>> = Vec::new();
    let mut fake = |base: &[u8]| {
        seen.push(base.to_vec());
        Ok(with_control(
            HB_ME,
            &[&lawyers, &given],
            DACL_AUTO_INHERITED,
        ))
    };

    let write = taken_write(&below, &others(), &me(), &mut fake).unwrap();
    let fresh = with_control(HB_ME, &[&lawyers, &given], DACL_AUTO_INHERITED);
    let again = taken_write(&fresh, &others(), &me(), &mut fake).unwrap();

    assert_eq!(again, None, "nothing stale: no write");
    assert_eq!(seen.len(), 1, "worked out once, for the stale item only");
    let base = Descriptor::parse(&seen[0]).unwrap();
    assert_eq!(
        base.dacl,
        Some(render(2, &[&lawyers]).as_slice()),
        "explicit entries alone"
    );
    let Some(TakenWrite::Inheriting(new)) = write else {
        panic!("expected one inheriting write, got {write:?}");
    };
    let read = Descriptor::parse(&new).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&lawyers, &given]).as_slice()));
}
