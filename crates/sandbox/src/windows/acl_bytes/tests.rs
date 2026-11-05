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
    let only = build_descriptor(0, Some(&dacl), None);
    let read = Descriptor::parse(&only).unwrap();
    assert_eq!(read.sacl, None);
    assert!(!read.dacl_protected());
    assert!(Descriptor::parse(&only[..10]).is_err());
    assert!(Acl::parse(&dacl[..dacl.len() - 1]).is_err());
}
