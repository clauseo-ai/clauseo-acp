use std::path::Path;

use super::*;
use crate::windows::acl_plan::{LOW_LABEL_SID, Rights};
use crate::windows::admit::PathKind;

fn sid(text: &str) -> Sid {
    Sid::parse(text).unwrap()
}

fn grant(sid_text: &str, rights: Rights, inherit: Inherit) -> Grant {
    Grant {
        path: Path::new("/m").to_owned(),
        sid: sid(sid_text),
        rights,
        inherit,
        kind: PathKind::Matter,
    }
}

const POOL: &str = "S-1-5-21-1-2-3-1001";
const OTHER: &str = "S-1-5-21-1-2-3-1002";

fn spec(sid_text: &str, rights: Rights) -> AceSpec {
    AceSpec::for_grant(&grant(sid_text, rights, Inherit::Tree), true).unwrap()
}

/// An entry as Windows would have inherited it.
fn inherited(sid_text: &str, rights: Rights) -> Vec<u8> {
    let mut spec = spec(sid_text, rights);
    spec.flags |= INHERITED;
    spec.bytes()
}

#[test]
fn sids_go_to_bytes_and_back() {
    for text in [POOL, LOW_LABEL_SID, "S-1-1-0", "S-1-5-32-545"] {
        let bytes = sid_bytes(&sid(text));
        assert_eq!(sid_text(&bytes), Some(sid(text)), "{text}");
    }
    // S-1-5-32-545: revision 1, two sub-authorities, authority 5, then 32 and 545.
    assert_eq!(
        sid_bytes(&sid("S-1-5-32-545")),
        [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 0x21, 2, 0, 0]
    );
}

#[test]
fn entry_flags_follow_its_inheritance_and_target() {
    let tree = grant(POOL, Rights::Modify, Inherit::Tree);
    assert_eq!(AceSpec::for_grant(&tree, true).unwrap().flags, 0x3);
    assert_eq!(
        AceSpec::for_grant(&tree, false).unwrap().flags,
        0,
        "a file inherits nothing"
    );
    let files = grant(OTHER, Rights::DenyRead, Inherit::FilesInFolder);
    assert_eq!(
        AceSpec::for_grant(&files, true).unwrap().flags,
        0x1 | 0x8 | 0x4
    );
    assert!(AceSpec::for_grant(&files, false).is_err());
    let deny = AceSpec::for_grant(&grant(OTHER, Rights::DenyReadWrite, Inherit::Tree), true);
    assert_eq!(deny.unwrap().kind, DENIED);
    let label = AceSpec::for_grant(&grant(LOW_LABEL_SID, Rights::LowLabel, Inherit::Tree), true);
    let label = label.unwrap();
    assert_eq!((label.kind, label.mask), (LABEL, 0x1));
}

#[test]
fn an_entry_goes_where_windows_order_puts_it_and_comes_out_exactly() {
    let base_allow = spec("S-1-5-18", Rights::Modify).bytes();
    let base_inherited = inherited("S-1-5-32-545", Rights::ReadAndExecute);
    let acl = render(2, &[&base_allow, &base_inherited]);
    let parsed = Acl::parse(&acl).unwrap();

    let deny = spec(OTHER, Rights::DenyReadWrite);
    let with_deny = parsed.with(&deny);
    let after = Acl::parse(&with_deny).unwrap();
    assert_eq!(
        after.aces()[0],
        deny.bytes().as_slice(),
        "an explicit deny goes first"
    );
    assert!(after.has(&deny));

    let allow = spec(POOL, Rights::Modify);
    let with_both = after.with(&allow);
    let both = Acl::parse(&with_both).unwrap();
    let order: Vec<&[u8]> = both.aces().to_vec();
    assert_eq!(order[0], deny.bytes().as_slice());
    assert_eq!(order[1], base_allow.as_slice());
    assert_eq!(
        order[2],
        allow.bytes().as_slice(),
        "an explicit allow before inherited ones"
    );
    assert_eq!(order[3], base_inherited.as_slice());

    let back = Acl::parse(&with_both).unwrap().without(&allow).unwrap();
    assert_eq!(back, with_deny, "removed exactly, nothing else moved");
    assert_eq!(
        Acl::parse(&acl).unwrap().without(&allow),
        None,
        "absent: no change"
    );
    // An inherited copy is never the explicit entry.
    let copy = inherited(POOL, Rights::Modify);
    let only_copy = render(2, &[&copy]);
    assert!(!Acl::parse(&only_copy).unwrap().has(&allow));
}

#[test]
fn a_new_label_replaces_an_explicit_one_and_keeps_inherited_ones() {
    let medium = AceSpec {
        kind: LABEL,
        flags: 0,
        mask: 1,
        sid: sid_bytes(&sid("S-1-16-8192")),
    };
    let low = spec(LOW_LABEL_SID, Rights::LowLabel);
    let sacl = render(2, &[&medium.bytes()]);
    let out = Acl::parse(&sacl).unwrap().with(&low);
    let out = Acl::parse(&out).unwrap();
    assert_eq!(out.aces().len(), 1);
    assert!(out.has(&low));
}

#[test]
fn an_explicit_allow_for_a_broad_group_or_a_watched_sid_beats_an_inherited_deny() {
    let everyone = spec("S-1-1-0", Rights::ReadAndExecute).bytes();
    let acl = render(2, &[&everyone]);
    assert!(broad_allow(&Acl::parse(&acl).unwrap(), &[]));

    let users_inherited = inherited("S-1-5-32-545", Rights::ReadAndExecute);
    let acl = render(2, &[&users_inherited]);
    assert!(
        !broad_allow(&Acl::parse(&acl).unwrap(), &[]),
        "inherited: the deny comes first"
    );

    let group = "S-1-5-21-1-2-3-1010";
    let acl = render(2, &[&spec(group, Rights::ReadAndExecute).bytes()]);
    let watched = [sid_bytes(&sid(group))];
    assert!(!broad_allow(&Acl::parse(&acl).unwrap(), &[]));
    assert!(broad_allow(&Acl::parse(&acl).unwrap(), &watched));

    let mut sync = spec("S-1-5-11", Rights::ReadAndExecute);
    sync.mask = 0x0010_0000;
    let acl = render(2, &[&sync.bytes()]);
    assert!(
        !broad_allow(&Acl::parse(&acl).unwrap(), &[]),
        "SYNCHRONIZE alone reads nothing"
    );

    let deny = render(2, &[&spec("S-1-1-0", Rights::DenyRead).bytes()]);
    assert!(!broad_allow(&Acl::parse(&deny).unwrap(), &[]));
}

/// An `ACCESS_ALLOWED_OBJECT_ACE` with no object type for `sid_text`: header, mask, the
/// object flags (none), then the SID. Windows checks it as a plain allow (L1).
fn object_allow(sid_text: &str) -> Vec<u8> {
    let sid = sid_bytes(&sid(sid_text));
    let size = u16::try_from(12 + sid.len()).unwrap();
    let mut ace = vec![5, 0];
    ace.extend_from_slice(&size.to_le_bytes());
    ace.extend_from_slice(&0x0012_00a9_u32.to_le_bytes());
    ace.extend_from_slice(&0_u32.to_le_bytes());
    ace.extend_from_slice(&sid);
    ace
}

#[test]
fn an_explicit_object_allow_beats_an_inherited_deny_whoever_it_names() {
    for named in ["S-1-1-0", "S-1-5-21-9-9-9-1001"] {
        let acl = render(2, &[&object_allow(named)]);
        assert!(broad_allow(&Acl::parse(&acl).unwrap(), &[]), "{named}");
    }
    let mut inherited = object_allow("S-1-1-0");
    inherited[1] = INHERITED;
    let acl = render(2, &[&inherited]);
    assert!(!broad_allow(&Acl::parse(&acl).unwrap(), &[]));
}

#[test]
fn an_explicit_allow_for_the_pool_machines_none_group_beats_an_inherited_deny() {
    let none = "S-1-5-21-1-2-3-513";
    let acl = render(2, &[&spec(none, Rights::ReadAndExecute).bytes()]);
    let pool_user = [sid_bytes(&sid("S-1-5-21-1-2-3-1005"))];
    assert!(broad_allow(&Acl::parse(&acl).unwrap(), &pool_user));
    let elsewhere = [sid_bytes(&sid("S-1-5-21-7-8-9-1005"))];
    assert!(!broad_allow(&Acl::parse(&acl).unwrap(), &elsewhere));
}

#[test]
fn the_explicit_label_is_read_back_as_written() {
    let medium = Label {
        level: 8192,
        flags: OBJECT_INHERIT,
        mask: 0x3,
    };
    let inherited = {
        let mut low = label_spec(Label {
            level: 4096,
            flags: 0,
            mask: 1,
        })
        .bytes();
        low[1] = INHERITED;
        low
    };
    let sacl = render(2, &[&inherited, &label_spec(medium).bytes()]);
    assert_eq!(explicit_label(&Acl::parse(&sacl).unwrap()), Some(medium));
    let only_inherited = render(2, &[&inherited]);
    assert_eq!(explicit_label(&Acl::parse(&only_inherited).unwrap()), None);
    let ours = spec(LOW_LABEL_SID, Rights::LowLabel);
    let ours_back =
        label_spec(explicit_label(&Acl::parse(&render(2, &[&ours.bytes()])).unwrap()).unwrap());
    assert_eq!(ours_back, ours);
}

/// M5: the repair of a child a pool user owns strips exactly the allows a strict walk refuses,
/// and the rebuilt descriptor keeps its owner and group, drops the protection, and reads back.
#[test]
fn a_repair_strips_the_offending_allows_and_keeps_the_owner() {
    let pool_user = "S-1-5-21-1-2-3-1005";
    let keep = spec(pool_user, Rights::Modify).bytes();
    let everyone = spec("S-1-1-0", Rights::ReadAndExecute).bytes();
    let other = spec("S-1-5-21-1-2-3-1006", Rights::Modify).bytes();
    let dacl = render(
        2,
        &[&keep, &everyone, &object_allow("S-1-5-21-9-9-9-9"), &other],
    );
    let watched = [sid_bytes(&sid("S-1-5-21-1-2-3-1006"))];
    let cleaned = without_broad(&Acl::parse(&dacl).unwrap(), &watched);
    let cleaned = Acl::parse(&cleaned).unwrap();
    assert_eq!(cleaned.aces(), &[keep.as_slice()]);
    assert!(!broad_allow(&cleaned, &watched));

    let owner = sid_bytes(&sid(pool_user));
    let me = sid_bytes(&sid("S-1-5-21-7-8-9-1001"));
    let before = rebuild(
        &build_descriptor(DACL_PROTECTED, Some(&dacl), None),
        Some(&owner),
        None,
        0,
    )
    .unwrap();
    assert_eq!(owner_bytes(&before), Some(owner.as_slice()));
    let repaired = rebuild(&before, None, Some(&render(2, &[&keep])), DACL_PROTECTED).unwrap();
    let read = Descriptor::parse(&repaired).unwrap();
    assert!(!read.dacl_protected());
    assert_eq!(owner_bytes(&repaired), Some(owner.as_slice()));
    assert_eq!(
        Acl::parse(read.dacl.unwrap()).unwrap().aces(),
        &[keep.as_slice()]
    );
    let reowned = rebuild(&repaired, Some(&me), None, 0).unwrap();
    assert_eq!(owner_bytes(&reowned), Some(me.as_slice()));
    assert_eq!(owner_descriptor(&me).len(), 20 + me.len());
    assert_eq!(owner_bytes(&owner_descriptor(&me)), Some(me.as_slice()));
}

#[test]
fn a_child_inherits_its_parents_low_label_unless_it_has_its_own() {
    let low = spec(LOW_LABEL_SID, Rights::LowLabel);
    let parent = render(2, &[&low.bytes()]);
    let parent = Acl::parse(&parent).unwrap();

    let folder = inherited_label(Some(&parent), None, 0, true).unwrap();
    let folder = Acl::parse(&folder).unwrap();
    assert_eq!(
        flags(folder.aces()[0]),
        OBJECT_INHERIT | CONTAINER_INHERIT | INHERITED
    );
    let file = inherited_label(Some(&parent), None, 0, false).unwrap();
    assert_eq!(flags(Acl::parse(&file).unwrap().aces()[0]), INHERITED);

    // Already inherited: nothing to write.
    let again = inherited_label(Some(&parent), Some(&folder), 0, true);
    assert_eq!(again, None);

    // Its own label stays; a protected SACL stays.
    let own = render(2, &[&AceSpec { flags: 0, ..low }.bytes()]);
    let own = Acl::parse(&own).unwrap();
    assert_eq!(inherited_label(None, Some(&own), 0, false), None);
    assert_eq!(
        inherited_label(None, Some(&folder), SACL_PROTECTED, true),
        None
    );

    // The parent's label gone: the inherited copy goes too.
    let gone = inherited_label(None, Some(&folder), 0, true).unwrap();
    assert_eq!(Acl::parse(&gone).unwrap().aces().len(), 0);
}

/// The Windows run: a DACL written auto-inherited asks to stay so (`SE_DACL_AUTO_INHERIT_REQ`,
/// `0x0100`), and protection is kept as given; one not auto-inherited doesn't ask.
#[test]
fn an_auto_inherited_dacl_is_written_with_the_request_to_keep_it() {
    let dacl = render(2, &[&spec(POOL, Rights::Modify).bytes()]);
    let control = |bytes: &[u8]| u16::from_le_bytes([bytes[2], bytes[3]]);
    let kept = build_descriptor(DACL_AUTO_INHERITED, Some(&dacl), None);
    assert_eq!(control(&kept), 0x8000 | 0x0400 | 0x0100 | 0x0004);
    let protected = build_descriptor(DACL_PROTECTED | DACL_AUTO_INHERITED, Some(&dacl), None);
    assert_eq!(
        control(&protected),
        0x8000 | 0x1000 | 0x0400 | 0x0100 | 0x0004
    );
    assert_eq!(
        control(&build_descriptor(0, Some(&dacl), None)),
        0x8000 | 0x0004
    );
    let sacl = render(2, &[&spec(LOW_LABEL_SID, Rights::LowLabel).bytes()]);
    let label = build_descriptor(DACL_AUTO_INHERITED, None, Some(&sacl));
    assert_eq!(control(&label) & 0x0100, 0, "no request without a DACL");
}

#[test]
fn a_descriptor_is_built_and_read_back() {
    let dacl = render(2, &[&spec(POOL, Rights::Modify).bytes()]);
    let sacl = render(2, &[&spec(LOW_LABEL_SID, Rights::LowLabel).bytes()]);
    let bytes = build_descriptor(
        DACL_PROTECTED | DACL_AUTO_INHERITED,
        Some(&dacl),
        Some(&sacl),
    );
    let read = Descriptor::parse(&bytes).unwrap();
    assert_eq!(read.dacl, Some(dacl.as_slice()));
    assert_eq!(read.sacl, Some(sacl.as_slice()));
    assert!(read.dacl_protected());
    assert_eq!(owner_sid(&bytes), None);
    let only = build_descriptor(0, Some(&dacl), None);
    let read = Descriptor::parse(&only).unwrap();
    assert_eq!(read.sacl, None);
    assert!(!read.dacl_protected());
    assert!(Descriptor::parse(&only[..10]).is_err());
    assert!(Acl::parse(&dacl[..dacl.len() - 1]).is_err());
}

/// The Windows run 2, row 15 (a plain `C:\m`): a child not marked auto-inherited loses its
/// inherited copies before it is worked out again, so none comes back explicit; its explicit
/// entries, owner, group and SACL stay, and the DACL is marked auto-inherited.
#[test]
fn a_dacl_not_auto_inherited_loses_its_inherited_copies_before_the_walk_works_it_out() {
    let owner = sid_bytes(&sid(POOL));
    let group = sid_bytes(&sid(OTHER));
    let explicit = spec(POOL, Rights::Modify).bytes();
    let users = inherited("S-1-5-32-545", Rights::ReadAndExecute);
    let dacl = render(2, &[&explicit, &users]);
    let sacl = render(2, &[&spec(LOW_LABEL_SID, Rights::LowLabel).bytes()]);
    let before = assemble(0, [Some(&owner), Some(&group), Some(&sacl), Some(&dacl)]);

    let after = without_inherited(&before, &empty_acl()).unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&explicit]).as_slice()));
    assert_eq!(read.sacl, Some(sacl.as_slice()));
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);
    let sid_at = |at: usize| {
        let offset = usize::try_from(read_u32(&after, at).unwrap()).unwrap();
        after[offset..offset + 8 + 4 * usize::from(after[offset + 1])].to_vec()
    };
    assert_eq!(sid_at(4), owner);
    assert_eq!(sid_at(8), group);
    let bare = build_descriptor(0, None, Some(&sacl));
    assert_eq!(without_inherited(&bare, &empty_acl()).unwrap(), bare);
}

/// An entry of `kind` with `flags`, `mask` and `sid_text`, as bytes.
fn ace(kind: u8, flags: u8, mask: u32, sid_text: &str) -> Vec<u8> {
    AceSpec {
        kind,
        flags,
        mask,
        sid: sid_bytes(&sid(sid_text)),
    }
    .bytes()
}

/// The Windows run 3, row 15: under a plain `C:\m`, NTFS writes a child's inherited copies with
/// flags `00`. Explicit entries equal but for the inherited flag to what the parent gives (on a
/// file, its object-inherit entries with the flags cleared; on a folder, as inherited onto a
/// container), or to an inherited entry the child holds, go; the rest stay.
#[test]
fn explicit_copies_of_inherited_entries_are_dropped_and_the_rest_kept() {
    let (owner, users, everyone) = (POOL, "S-1-5-32-545", "S-1-1-0");
    let due = render(
        2,
        &[
            &ace(ALLOWED, INHERITED, 0x001f_01ff, "S-1-5-18"),
            &ace(ALLOWED, INHERITED, 0x0012_00a9, users),
            &ace(ALLOWED, INHERITED, 0x001f_01ff, owner),
            // Not marked inherited: a default DACL's entry, not the parent's.
            &ace(ALLOWED, 0, 0x0012_00a9, everyone),
        ],
    );
    let copies = [
        ace(ALLOWED, 0, 0x001f_01ff, "S-1-5-18"),
        ace(ALLOWED, 0, 0x0012_00a9, users),
        ace(ALLOWED, 0, 0x001f_01ff, owner),
    ];
    let kept = [
        // Equal to the child's own stale inherited deny, which the parent no longer gives:
        // the lawyer's, kept (round 10).
        ace(DENIED, 0, 0x1, OTHER),
        ace(ALLOWED, 0, 0x0012_00a9, everyone),
        ace(ALLOWED, 0, 0x0012_0089, users),
        ace(ALLOWED, OBJECT_INHERIT, 0x001f_01ff, "S-1-5-18"),
    ];
    let held = ace(DENIED, INHERITED, 0x1, OTHER);
    let mut all: Vec<&[u8]> = copies.iter().chain(&kept).map(Vec::as_slice).collect();
    all.push(&held);
    let before = build_descriptor(0, Some(&render(2, &all)), None);
    let after = without_inherited(&before, &due).unwrap();
    let want: Vec<&[u8]> = kept.iter().map(Vec::as_slice).collect();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(render(2, &want).as_slice()));
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);

    let folder_due = render(2, &[&ace(ALLOWED, 0x3 | INHERITED, 0x001f_01ff, users)]);
    let on_folder = ace(ALLOWED, 0x3, 0x001f_01ff, users);
    let files_only = ace(ALLOWED, OBJECT_INHERIT, 0x001f_01ff, users);
    let before = build_descriptor(0, Some(&render(2, &[&on_folder, &files_only])), None);
    let after = without_inherited(&before, &folder_due).unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&files_only]).as_slice()));
}

/// Round 8: what a new child's descriptor is worked out from keeps the owner and group and has
/// an empty DACL, marked auto-inherited.
#[test]
fn a_bare_child_keeps_its_owner_and_has_no_entries() {
    let owner = sid_bytes(&sid(POOL));
    let dacl = render(2, &[&spec(POOL, Rights::Modify).bytes()]);
    let before = assemble(0, [Some(&owner), None, None, Some(&dacl)]);
    let bare = bare_child(&before).unwrap();
    let read = Descriptor::parse(&bare).unwrap();
    assert_eq!(read.dacl, Some(empty_acl().as_slice()));
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);
    let offset = usize::try_from(read_u32(&bare, 4).unwrap()).unwrap();
    assert_eq!(&bare[offset..offset + owner.len()], owner.as_slice());
}

/// Round 9: an auto-inherited child's explicit entry is one somebody wrote, so it stays even
/// when it equals an inheritable entry of the parent; on a child not auto-inherited (where NTFS
/// writes flagless copies) the same entry goes. Inherited entries go from both.
/// Round 10: an explicit deny equal to an inherited entry the child still holds, but which its
/// parent no longer gives, is the lawyer's and stays explicit; only what the parent gives now
/// (`due`) makes an explicit entry a copy.
#[test]
fn an_explicit_deny_equal_only_to_a_stale_inherited_entry_stays() {
    let deny = ace(DENIED, 0, 0x0001_0000, OTHER);
    let stale = ace(DENIED, INHERITED, 0x0001_0000, OTHER);
    let due = render(2, &[&ace(ALLOWED, INHERITED, 0x0012_00a9, "S-1-5-32-545")]);
    let plain = build_descriptor(0, Some(&render(2, &[&deny, &stale])), None);
    let after = without_inherited(&plain, &due).unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&deny]).as_slice()));
}

#[test]
fn an_auto_inherited_child_keeps_an_explicit_entry_equal_to_an_inherited_one() {
    let users = "S-1-5-32-545";
    let due = render(2, &[&ace(ALLOWED, INHERITED, 0x0012_00a9, users)]);
    let explicit = ace(ALLOWED, 0, 0x0012_00a9, users);
    let held = ace(ALLOWED, INHERITED, 0x001f_01ff, OTHER);
    let dacl = render(2, &[&explicit, &held]);

    let ai = build_descriptor(DACL_AUTO_INHERITED, Some(&dacl), None);
    let after = without_inherited(&ai, &due).unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(render(2, &[&explicit]).as_slice()));
    assert_eq!(read.control & DACL_AUTO_INHERITED, DACL_AUTO_INHERITED);

    let plain = build_descriptor(0, Some(&dacl), None);
    let after = without_inherited(&plain, &due).unwrap();
    let read = Descriptor::parse(&after).unwrap();
    assert_eq!(read.dacl, Some(empty_acl().as_slice()));
}
