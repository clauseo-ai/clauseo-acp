//! What `--reclaim` took, cleaned: only the explicit allows of pool users other than the
//! matter's own go, and the explicit denies of the lawyer and its token's groups (in a protected
//! DACL, one marked inherited too: that is forged); a broad group's allow, the matter's own pool
//! user's, inherited entries of an unprotected DACL, a pool user's deny and the DACL's
//! protection stay. Nothing else the lawyer owns is touched.

use super::*;

fn others() -> [Vec<u8>; 1] {
    [sid_bytes(&sid(HB_OTHER))]
}

fn me() -> Vec<u8> {
    sid_bytes(&sid(HB_ME))
}

/// The desktop user and a group of its token, as the hand-back passes them.
fn mine() -> [Vec<u8>; 2] {
    [me(), sid_bytes(&sid("S-1-1-0"))]
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

    let after = stripped(&before, &others(), &me(), &mine())
        .unwrap()
        .unwrap();

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
        stripped(&read_back(owner, aces), &others(), &me(), &mine())
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
        stripped(&read_back(HB_ME, &[&other]), &others(), &[], &mine())
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
    let after = stripped(&shut, &others(), &me(), &mine()).unwrap().unwrap();
    assert!(bare(&after).unwrap());

    let colleague = spec("S-1-5-21-9-9-9-1500", Rights::ReadAndExecute).bytes();
    let shared = with_control(HB_ME, &[&other, &colleague], DACL_PROTECTED);
    let after = stripped(&shared, &others(), &me(), &mine())
        .unwrap()
        .unwrap();
    assert!(
        !bare(&after).unwrap(),
        "a colleague's entry keeps the protection"
    );

    let open = with_control(HB_ME, &[&other], 0);
    let after = stripped(&open, &others(), &me(), &mine()).unwrap().unwrap();
    assert!(!bare(&after).unwrap(), "not protected: nothing to undo");
}

/// An agent forges an "inherited" allow in a protected DACL (`D:P(A;ID;FA;;;<pool user>)`):
/// nothing is inherited through a protected DACL, so it goes as an explicit one would, and the
/// item is left bare. In an unprotected DACL a real inherited entry stays.
#[test]
fn a_forged_inherited_allow_in_a_protected_dacl_goes() {
    let forged = inherited(HB_OTHER, Rights::Modify);
    let shut = with_control(HB_ME, &[&forged], DACL_PROTECTED);
    let after = stripped(&shut, &others(), &me(), &mine()).unwrap().unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(empty_acl().as_slice()));
    assert!(bare(&after).unwrap());

    let real = with_control(HB_ME, &[&forged], DACL_AUTO_INHERITED);
    assert!(
        stripped(&real, &others(), &me(), &mine())
            .unwrap()
            .is_none()
    );
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

    let write = taken_write(&shut, &others(), &me(), &mine(), &mine(), &mut fake).unwrap();

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
    let write = taken_write(&shared, &others(), &me(), &mine(), &mine(), &mut fake).unwrap();
    let Some(TakenWrite::Stripped(new)) = write else {
        panic!("expected a strip, got {write:?}");
    };
    let read = Descriptor::parse(&new).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&colleague]).as_slice()));
    assert_eq!(read.control & DACL_PROTECTED, DACL_PROTECTED);

    let clean = with_control(HB_ME, &[&colleague], DACL_PROTECTED);
    assert_eq!(
        taken_write(&clean, &others(), &me(), &mine(), &mine(), &mut fake).unwrap(),
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

    let write = taken_write(&below, &others(), &me(), &mine(), &mine(), &mut fake).unwrap();
    let fresh = with_control(HB_ME, &[&lawyers, &given], DACL_AUTO_INHERITED);
    let again = taken_write(&fresh, &others(), &me(), &mine(), &mine(), &mut fake).unwrap();

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

/// `WRITE_OWNER`.
const WO: u32 = 0x0008_0000;

/// A hijacked agent's denies of the lawyer and of a group of the lawyer's token
/// (`(D;;WO;;;lawyer)`, `(D;;FA;;;WD)`) go with the other pool user's allow, so the item can't
/// later refuse the lawyer's label write; a deny naming a pool user (our fence) stays, even when
/// that SID is passed as the lawyer's too.
#[test]
fn the_lawyers_and_its_groups_denies_go_and_a_pool_users_deny_stays() {
    let lawyer = ace(DENIED, 0, WO, HB_ME);
    let everyone = ace(DENIED, 0, 0x001f_01ff, "S-1-1-0");
    let fence = ace(DENIED, 0, 0x001f_01ff, HB_OTHER);
    let own = spec(HB_OWN, Rights::Modify).bytes();
    let other = spec(HB_OTHER, Rights::Modify).bytes();
    let all: [&[u8]; 5] = [&lawyer, &everyone, &fence, &own, &other];
    let before = with_control(HB_ME, &all, DACL_AUTO_INHERITED);

    let after = stripped(&before, &others(), &me(), &mine())
        .unwrap()
        .unwrap();

    let want: [&[u8]; 2] = [&fence, &own];
    assert_eq!(
        Descriptor::parse(&after).unwrap().dacl,
        Some(render(2, &want).as_slice())
    );
    let fenced = with_control(HB_ME, &[&fence], DACL_AUTO_INHERITED);
    let both = [me(), sid_bytes(&sid(HB_OTHER))];
    assert!(
        stripped(&fenced, &others(), &me(), &both)
            .unwrap()
            .is_none(),
        "a pool user's deny stays"
    );
    let only_lawyer = with_control(HB_ME, &[&lawyer, &own], DACL_AUTO_INHERITED);
    let after = stripped(&only_lawyer, &others(), &me(), &mine())
        .unwrap()
        .unwrap();
    assert_eq!(
        Descriptor::parse(&after).unwrap().dacl,
        Some(render(2, &[&own]).as_slice()),
        "a deny alone is reason enough to write"
    );
}

/// Below a folder the agent gave an inheritable deny of the lawyer, the inherited copy goes as
/// the agent's allow copies do: the item inherits again, from its explicit entries alone.
#[test]
fn an_inherited_copy_of_the_agents_deny_of_the_lawyer_inherits_again() {
    let copy = ace(DENIED, INHERITED, WO, HB_ME);
    let lawyers = spec(HB_ME, Rights::Modify).bytes();
    let below = with_control(HB_ME, &[&copy, &lawyers], DACL_AUTO_INHERITED);
    let given = inherited(HB_OWN, Rights::Modify);
    let mut fake = |_: &[u8]| {
        Ok(with_control(
            HB_ME,
            &[&lawyers, &given],
            DACL_AUTO_INHERITED,
        ))
    };

    let write = taken_write(&below, &others(), &me(), &mine(), &mine(), &mut fake).unwrap();

    let Some(TakenWrite::Inheriting(new)) = write else {
        panic!("expected one inheriting write, got {write:?}");
    };
    let want: [&[u8]; 2] = [&lawyers, &given];
    assert_eq!(
        Descriptor::parse(&new).unwrap().dacl,
        Some(render(2, &want).as_slice())
    );
}

/// A deny object entry (`ACCESS_DENIED_OBJECT_ACE_TYPE`) naming `sid`, with no object type.
fn denied_object(mask: u32, sid_text: &str) -> Vec<u8> {
    let sid = sid_bytes(&sid(sid_text));
    let size = u16::try_from(12 + sid.len()).unwrap();
    let mut out = vec![6, 0];
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&mask.to_le_bytes());
    out.extend_from_slice(&0_u32.to_le_bytes());
    out.extend_from_slice(&sid);
    out
}

/// On a taken item, `OWNER RIGHTS`' deny goes when the caller lists it as the lawyer's, as the
/// NTFS writer does (with `CREATOR OWNER` and `CREATOR GROUP`), and so does a deny object entry
/// naming the lawyer, which an access check without object types applies as a plain deny. With
/// nothing listed (an item `--reclaim` didn't take) every deny stays.
#[test]
fn owner_rights_and_a_deny_object_entry_go_on_a_taken_item_and_nothing_listed_keeps_all() {
    let owner_rights = ace(DENIED, 0, WO, "S-1-3-4");
    let creator = ace(DENIED, 0x1 | 0x2 | 0x8, WO, "S-1-3-0");
    let object = denied_object(WO, HB_ME);
    let own = spec(HB_OWN, Rights::Modify).bytes();
    let all: [&[u8]; 4] = [&owner_rights, &creator, &object, &own];
    let before = with_control(HB_ME, &all, DACL_AUTO_INHERITED);
    let lawyers = [
        me(),
        sid_bytes(&sid("S-1-3-0")),
        sid_bytes(&sid("S-1-3-1")),
        sid_bytes(&sid("S-1-3-4")),
    ];

    let after = stripped(&before, &others(), &me(), &lawyers)
        .unwrap()
        .unwrap();

    assert_eq!(
        Descriptor::parse(&after).unwrap().dacl,
        Some(render(2, &[&own]).as_slice())
    );
    let lawyer = ace(DENIED, 0, 0x0001_0000, HB_ME);
    let everyone = ace(DENIED, 0, 0x0001_0000, "S-1-1-0");
    let kept = with_control(
        HB_ME,
        &[&lawyer, &everyone, &object, &own],
        DACL_AUTO_INHERITED,
    );
    assert!(
        stripped(&kept, &others(), &me(), &[]).unwrap().is_none(),
        "not taken: its denies stay"
    );
}

/// What `--reclaim` writes before the owner: every `OWNER RIGHTS` entry goes (deny and allow,
/// explicit and marked inherited, plain and object), and everything else stays byte for byte, in
/// its order, with the DACL's revision and control bits. None when there is none.
#[test]
fn the_exe_takes_off_only_owner_rights_entries_and_leaves_every_other_byte() {
    let everyone = ace(DENIED, 0, 0x001f_01ff, "S-1-1-0");
    let ow_deny = ace(DENIED, 0, WO, "S-1-3-4");
    let pool = spec(HB_OTHER, Rights::Modify).bytes();
    let ow_allow = ace(ALLOWED, INHERITED, 0x0012_0089, "S-1-3-4");
    let ow_object = denied_object(WO, "S-1-3-4");
    let lawyer = inherited(HB_ME, Rights::Modify);
    let all: [&[u8]; 6] = [&everyone, &ow_deny, &pool, &ow_allow, &ow_object, &lawyer];
    for control in [DACL_PROTECTED, DACL_AUTO_INHERITED] {
        let before = with_control(HB_OTHER, &all, control);

        let after = without_owner_rights(&before).unwrap().unwrap();

        let read = Descriptor::parse(&after).unwrap();
        let want: [&[u8]; 3] = [&everyone, &pool, &lawyer];
        assert_eq!(read.dacl, Some(render(2, &want).as_slice()));
        let bits = DACL_PROTECTED | DACL_AUTO_INHERITED;
        let was = Descriptor::parse(&before).unwrap().control & bits;
        assert_eq!(read.control & bits, was);
        assert_eq!(owner_bytes(&after), None, "the DACL alone is written");
    }
    let clean = with_control(HB_OTHER, &[&everyone, &pool], DACL_PROTECTED);
    assert_eq!(without_owner_rights(&clean).unwrap(), None);
}

/// Below a taken folder, a file the lawyer always owned keeps its explicit denies (nothing of it
/// was taken) but loses an inherited copy of the agent's deny of the lawyer that its folder no
/// longer gives: it inherits again, from its explicit entries alone.
#[test]
fn below_a_taken_folder_only_stale_inherited_copies_go() {
    let copy = ace(DENIED, INHERITED, WO, HB_ME);
    let own_deny = ace(DENIED, 0, 0x0001_0000, "S-1-1-0");
    let lawyers = spec(HB_ME, Rights::Modify).bytes();
    let below = with_control(HB_ME, &[&own_deny, &copy, &lawyers], DACL_AUTO_INHERITED);
    let given = inherited(HB_OWN, Rights::Modify);
    let mut fake = |base: &[u8]| {
        let explicit = Descriptor::parse(base).unwrap().dacl.map(<[u8]>::to_vec);
        assert_eq!(explicit, Some(render(2, &[&own_deny, &lawyers])));
        Ok(with_control(
            HB_ME,
            &[&own_deny, &lawyers, &given],
            DACL_AUTO_INHERITED,
        ))
    };

    let write = taken_write(&below, &others(), &me(), &[], &mine(), &mut fake).unwrap();

    let Some(TakenWrite::Inheriting(new)) = write else {
        panic!("expected one inheriting write, got {write:?}");
    };
    let want: [&[u8]; 3] = [&own_deny, &lawyers, &given];
    assert_eq!(
        Descriptor::parse(&new).unwrap().dacl,
        Some(render(2, &want).as_slice())
    );
    let fresh = with_control(HB_ME, &[&own_deny, &lawyers], DACL_AUTO_INHERITED);
    assert_eq!(
        taken_write(&fresh, &others(), &me(), &[], &mine(), &mut fake).unwrap(),
        None,
        "its own deny stays: no write"
    );
}
