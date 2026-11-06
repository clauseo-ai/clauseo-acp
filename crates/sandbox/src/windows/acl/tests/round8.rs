//! Round 8 of S15 PR3 on a real NTFS volume (the Windows run 3, row 15): a matter under a plain
//! `C:\m`, whose DACLs hold their inherited copies without the inherited flag; and issue #413's
//! matter on a data drive whose root isn't auto-inherited, as GitHub's runners' `D:\` is.

use super::*;
use crate::windows::acl_bytes::owner_sid;
use crate::windows::record::current_user_sid;
use crate::windows::test_acl::entries;
use std::fmt::Write as _;

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
/// `matter\sub\doc.txt` and `matter\a-locked`: the matter's copies of `C:\m`'s entries, and its
/// children's copies of the matter's, all without the inherited flag or the auto-inherited mark.
/// `SYSTEM` and `Administrators` full control and `Users` read and execute down the tree,
/// `Users`' two folder-only rights, and `CREATOR OWNER`'s full control, which reaches a child as
/// its owner's. Every path is written, none left as made: a child left as made holds what the
/// temp folder's chain gave it, and under a chain not auto-inherited that is flagless copies the
/// rewritten matter no longer passes down, which the strict walk rightly refuses (issue #413:
/// GitHub's `D:\` root passed `Authenticated Users` Modify down to `a-locked` that way). Returns
/// the matter.
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
    for rel in ["matter/sub", "matter/a-locked"] {
        let child = place.at(rel);
        raw_dacl(&child, &folder(Some(owner(&child))));
    }
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

/// Every path from `place`'s folder down (each folder before its children, by name), and the
/// folders above it to the drive root: owner, DACL control bits and entries, one line each, for
/// a failed launch's message. The folders above show what the temp folder passed down when the
/// fixture made the tree, which a child left as made (`a-locked`) still holds.
fn dump(place: &Place) -> String {
    let mut out = String::new();
    for above in place
        .root
        .ancestors()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        describe(&mut out, &above.display().to_string(), above);
    }
    let mut paths = vec![place.root.clone()];
    while let Some(path) = paths.pop() {
        let rel = path.strip_prefix(&place.root).unwrap_or(&path);
        describe(&mut out, &format!("place/{}", rel.display()), &path);
        if let Ok(read) = std::fs::read_dir(&path) {
            let mut names: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
            names.sort();
            paths.extend(names.into_iter().rev());
        }
    }
    out
}

/// One path of [`dump`]: its owner, control bits (`AI` auto-inherited, `P` protected) and
/// entries, or why it couldn't be read.
fn describe(out: &mut String, name: &str, path: &Path) {
    let bytes = match descriptor(path) {
        Ok(bytes) => bytes,
        Err(error) => {
            writeln!(out, "{name}: unreadable ({error})").unwrap();
            return;
        }
    };
    let control = Descriptor::parse(&bytes).map_or(0, |d| d.control);
    let ai = if control & DACL_AUTO_INHERITED != 0 {
        " AI"
    } else {
        ""
    };
    let p = if control & DACL_PROTECTED != 0 {
        " P"
    } else {
        ""
    };
    let owner = owner_sid(&bytes);
    writeln!(out, "{name}: owner {owner:?} control {control:#06x}{ai}{p}").unwrap();
    for (kind, flags, mask, sid) in entries(path) {
        writeln!(
            out,
            "  kind {kind} flags {flags:#04x} mask {mask:#010x} {}",
            sid.as_str()
        )
        .unwrap();
    }
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
    let mut acl = NtfsAcl::new().unwrap();
    let launched = put_with(&mut acl, &matter, &fence(&matter));
    if let Err(error) = launched {
        panic!(
            "the strict launch was refused with {error:?} at {:?}:\n{}",
            acl.refused_at(),
            dump(&place)
        );
    }
    let children = [
        "matter/a-locked",
        "matter/sub",
        "matter/sub/doc.txt",
        "matter/lawyer.txt",
    ];
    for rel in children {
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

/// `Authenticated Users`.
const AUTHENTICATED: &str = "S-1-5-11";

/// Writes on `place`'s folder, raw (not auto-inherited), the DACL a data drive's root has as
/// format leaves it, as GitHub's runners' `D:\` had it under their temp folder (issue #413):
/// `SYSTEM` and `Administrators` full control and `Users` read and execute down the tree,
/// `Authenticated Users`' Modify for what is below (generic rights, inherit only) and its
/// create-folders right on the root alone. This process's user gets full control down the
/// tree too, so a Medium token can make and walk the tree. What is made below it afterwards
/// holds NTFS's flagless copies, the matter included, whatever the temp folder above holds.
fn data_drive_root(place: &Place) {
    let sid = |text: &str| sid_bytes(&Sid::parse(text).unwrap());
    let tree = OBJECT_INHERIT | CONTAINER_INHERIT;
    let me = sid_bytes(&current_user_sid().unwrap());
    raw_dacl(
        &place.root,
        &[
            (sid("S-1-5-18"), tree, 0x001f_01ff),
            (sid("S-1-5-32-544"), tree, 0x001f_01ff),
            (me, tree, 0x001f_01ff),
            (sid(USERS), tree, 0x0012_00a9),
            (sid(AUTHENTICATED), tree | 0x8, 0xe001_0000),
            (sid(AUTHENTICATED), 0, 0x4),
        ],
    );
}

/// Issue #413, a lawyer's layout: a matter made on a data drive whose root isn't auto-inherited,
/// its children made in it, so every DACL in the tree holds NTFS's flagless copies, among them
/// `Authenticated Users`' Modify. Each copy is what the matter passes down, so the strict launch
/// passes, and every child ends auto-inherited with each entry once, inherited.
#[test]
fn a_strict_launch_on_a_matter_made_under_a_plain_data_drive_root_passes() {
    let place = Place::new("acl-plain-drive");
    data_drive_root(&place);
    place.dir.file("matter/lawyer.txt", "x");
    place.dir.file("matter/sub/doc.txt", "x");
    place.dir.dir("matter/empty");
    let lawyer = place.at("matter/lawyer.txt");
    assert!(
        !auto(&lawyer) && has(&lawyer, AUTHENTICATED, false),
        "NTFS made lawyer.txt with a flagless copy:\n{}",
        dump(&place)
    );
    let matter = place.at("matter");
    let mut acl = NtfsAcl::new().unwrap();
    if let Err(error) = put_with(&mut acl, &matter, &fence(&matter)) {
        panic!(
            "the strict launch was refused with {error:?} at {:?}:\n{}",
            acl.refused_at(),
            dump(&place)
        );
    }
    for rel in [
        "matter/empty",
        "matter/sub",
        "matter/sub/doc.txt",
        "matter/lawyer.txt",
    ] {
        let path = place.at(rel);
        assert!(auto(&path), "{rel} ends auto-inherited");
        assert_eq!(twice(&path), [], "{rel}");
        assert!(
            has(&path, AUTHENTICATED, true) && !has(&path, AUTHENTICATED, false),
            "{rel}:\n{}",
            dump(&place)
        );
        assert!(has(&path, OTHER_POOL_SIDS[0], true), "{rel} gets the fence");
    }
}

/// Issue #413, the CI failure made on purpose: as above, but the matter's DACL is replaced
/// without propagating after its children were made, so it no longer passes
/// `Authenticated Users`' Modify down. A child's flagless copy of it is then an explicit allow
/// the walk can't tell from one the lawyer wrote (round 10), and a broad one: the strict launch
/// refuses, naming that child.
#[test]
fn a_flagless_copy_the_matter_no_longer_passes_down_refuses_the_strict_launch() {
    let place = Place::new("acl-plain-stale");
    data_drive_root(&place);
    place.dir.dir("matter/made");
    let sid = |text: &str| sid_bytes(&Sid::parse(text).unwrap());
    let tree = OBJECT_INHERIT | CONTAINER_INHERIT;
    let me = sid_bytes(&current_user_sid().unwrap());
    // The matter's entries without Authenticated Users': every other copy on `made` is still
    // one the matter passes down.
    let matter = place.at("matter");
    raw_dacl(
        &matter,
        &[
            (sid("S-1-5-18"), tree, 0x001f_01ff),
            (sid("S-1-5-32-544"), tree, 0x001f_01ff),
            (me, tree, 0x001f_01ff),
            (sid(USERS), tree, 0x0012_00a9),
        ],
    );
    let made = place.at("matter/made");
    assert!(has(&made, AUTHENTICATED, false), "{}", dump(&place));
    let mut acl = NtfsAcl::new().unwrap();
    assert_eq!(
        put_with(&mut acl, &matter, &fence(&matter)),
        Err(ApplyFailure::BroadAllowChild)
    );
    let at = acl.refused_at().unwrap();
    assert!(at.ends_with("matter\\made"), "{at:?}");
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
