//! Round 8 of S15 PR3 on a real NTFS volume (the Windows run 3, row 15): a matter under a plain
//! `C:\m`, whose DACLs hold their inherited copies without the inherited flag.

use super::*;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::entries;

/// `CREATOR OWNER`.
const CREATOR_OWNER: &str = "S-1-3-0";

/// `path`'s owner, as bytes.
fn owner(path: &Path) -> Vec<u8> {
    let bytes = descriptor(path).unwrap();
    let offset = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    let offset = usize::try_from(offset).unwrap();
    bytes[offset..offset + 8 + 4 * usize::from(bytes[offset + 1])].to_vec()
}

/// Writes allows of (SID, flags, mask) as `path`'s DACL, raw: not auto-inherited, nothing
/// propagated.
fn raw_dacl(path: &Path, aces: &[(Vec<u8>, u8, u32)]) {
    raw_dacl_marked(path, 0, aces);
}

/// Writes allows of (SID, flags, mask) as `path`'s DACL, raw, with the control bits `control`.
fn raw_dacl_marked(path: &Path, control: u16, aces: &[(Vec<u8>, u8, u32)]) {
    let mut dacl = empty_acl();
    for (sid, flags, mask) in aces {
        let ace = AceSpec {
            kind: ALLOWED,
            flags: *flags,
            mask: *mask,
            sid: sid.clone(),
        };
        dacl = Acl::parse(&dacl).unwrap().with(&ace);
    }
    let written = build_descriptor(control, Some(&dacl), None);
    Object::open(path)
        .unwrap()
        .set(DACL_SECURITY_INFORMATION, &written)
        .unwrap();
}

/// A matter under a plain `C:\m` as the Windows run 3 found `M15d`, with `matter\lawyer.txt`,
/// `matter\sub\doc.txt` and `matter\a-locked` (left as made): the matter's copies of `C:\m`'s
/// entries, and its children's copies of the matter's, all without the inherited flag or the
/// auto-inherited mark. `SYSTEM` and `Administrators` full control and `Users` read and execute
/// down the tree, `Users`' two folder-only rights, and `CREATOR OWNER`'s full control, which
/// reaches a child as its owner's. Returns the matter.
fn plain_matter(place: &Place) -> PathBuf {
    place.dir.file("matter/lawyer.txt", "x");
    place.dir.file("matter/sub/doc.txt", "x");
    place.dir.dir("matter/a-locked");
    let sid = |text: &str| sid_bytes(&Sid::parse(text).unwrap());
    let tree = OBJECT_INHERIT | CONTAINER_INHERIT;
    let full = 0x001f_01ff;
    let read = 0x0012_00a9;
    let folder = |owner: Option<Vec<u8>>| {
        let mut aces = vec![
            (sid("S-1-5-18"), tree, full),
            (sid("S-1-5-32-544"), tree, full),
            (sid(USERS), tree, read),
            (sid(USERS), CONTAINER_INHERIT, 0x4),
            (sid(USERS), CONTAINER_INHERIT, 0x2),
            (sid(CREATOR_OWNER), tree | 0x8, 0x1000_0000),
        ];
        aces.extend(owner.map(|o| (o, 0, full)));
        aces
    };
    let matter = place.at("matter");
    raw_dacl(&matter, &folder(None));
    let sub = place.at("matter/sub");
    raw_dacl(&sub, &folder(Some(owner(&sub))));
    for rel in ["matter/lawyer.txt", "matter/sub/doc.txt"] {
        let file = place.at(rel);
        let aces = [
            (sid("S-1-5-18"), 0, full),
            (sid("S-1-5-32-544"), 0, full),
            (sid(USERS), 0, read),
            (owner(&file), 0, full),
        ];
        raw_dacl(&file, &aces);
    }
    matter
}

/// The entries `path` holds twice, explicit and inherited: equal but for the inherited flag.
fn twice(path: &Path) -> Vec<(u8, u8, u32, Sid)> {
    let all = entries(path);
    all.iter()
        .filter(|(kind, flags, mask, sid)| {
            flags & INHERITED == 0
                && all.iter().any(|(k, f, m, s)| {
                    f & INHERITED != 0
                        && k == kind
                        && m == mask
                        && s == sid
                        && f & !INHERITED == *flags
                })
        })
        .cloned()
        .collect()
}

/// The Windows run 3, row 15: the strict launch refused `BroadAllowChild` at `lawyer.txt`, whose
/// copy of the matter's `Users` allow NTFS wrote without the inherited flag. It passes now, and
/// every child ends auto-inherited holding each of the matter's entries once, inherited.
#[test]
fn a_strict_launch_on_a_plain_matter_passes_and_leaves_each_child_its_entries_once() {
    let place = Place::new("acl-plain-copies");
    let matter = plain_matter(&place);
    put(&matter, &fence(&matter)).unwrap();
    for rel in ["matter/sub", "matter/sub/doc.txt", "matter/lawyer.txt"] {
        let path = place.at(rel);
        assert!(auto(&path), "{rel} ends auto-inherited");
        assert_eq!(twice(&path), [], "{rel}");
        assert!(
            has(&path, USERS, true) && !has(&path, USERS, false),
            "{rel}"
        );
        assert!(has(&path, OTHER_POOL_SIDS[0], true), "{rel} gets the fence");
    }
}

/// The Windows run 3, row 15: a refused launch's rollback (its entries removed, the matter
/// walked plainly) left `lawyer.txt` auto-inherited with each copy twice, explicit and inherited.
/// Refused here by a protected folder listed before `lawyer.txt`, the rollback walk now leaves
/// it each entry once, inherited.
#[test]
fn the_rollback_walk_after_a_refused_launch_leaves_a_plain_child_each_entry_once() {
    let place = Place::new("acl-plain-rollback");
    let matter = plain_matter(&place);
    protect_with(&place.at("matter/a-locked"), "D:P(A;OICI;FA;;;OW)").unwrap();
    let grants = fence(&matter);
    assert_eq!(put(&matter, &grants), Err(ApplyFailure::ProtectedChild));
    let mut acl = NtfsAcl::new().unwrap();
    for g in &grants {
        acl.remove(g).unwrap();
    }
    acl.propagate(&matter, Reach::Tree, None).unwrap();
    let lawyer = place.at("matter/lawyer.txt");
    assert!(auto(&lawyer), "lawyer.txt ends auto-inherited");
    assert_eq!(twice(&lawyer), []);
    assert!(has(&lawyer, USERS, true) && !has(&lawyer, USERS, false));
}

/// Round 9: an auto-inherited child's explicit entry equal to one its parent passes down is one
/// somebody wrote, so the walk keeps it; a child not auto-inherited holds such an entry as
/// NTFS's flagless copy, and the walk takes it for the inherited one.
#[test]
fn an_auto_inherited_child_keeps_an_explicit_entry_its_parent_also_passes_down() {
    let place = Place::new("acl-ai-explicit");
    place.dir.file("matter/ai.txt", "x");
    place.dir.file("matter/plain.txt", "x");
    let sid = |text: &str| sid_bytes(&Sid::parse(text).unwrap());
    let (full, read) = (0x001f_01ff, 0x0012_00a9);
    let other = OTHER_POOL_SIDS[1];
    let matter = place.at("matter");
    let tree = OBJECT_INHERIT | CONTAINER_INHERIT;
    // The lawyer (this process's user) gets full control throughout, inherited as a lawyer's
    // matter would give it, so a Medium token can walk the tree (VM run 4 failed it there).
    let me = sid_bytes(&current_user_sid().unwrap());
    let passed = [
        (sid("S-1-5-18"), tree, full),
        (me.clone(), tree, full),
        (sid(other), tree, read),
    ];
    raw_dacl_marked(&matter, DACL_AUTO_INHERITED, &passed);
    let ai = place.at("matter/ai.txt");
    let held = [
        (sid(other), 0, read),
        (sid("S-1-5-18"), INHERITED, full),
        (me.clone(), INHERITED, full),
    ];
    raw_dacl_marked(&ai, DACL_AUTO_INHERITED, &held);
    let plain = place.at("matter/plain.txt");
    raw_dacl(
        &plain,
        &[
            (sid(other), 0, read),
            (sid("S-1-5-18"), 0, full),
            (me, 0, full),
        ],
    );

    NtfsAcl::new()
        .unwrap()
        .propagate(&matter, Reach::Tree, None)
        .unwrap();
    assert!(has(&ai, other, false), "ai.txt keeps its explicit entry");
    assert!(has(&ai, other, true), "ai.txt inherits the matter's");
    assert!(has(&plain, other, true) && !has(&plain, other, false));
}
