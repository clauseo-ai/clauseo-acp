//! The guard's repair of a child a pool user owns (`without_guarded`, #808's M1).

use super::*;

/// `OWNER RIGHTS`.
const OW: &str = "S-1-3-4";
/// The lawyer.
const LAWYER: &str = "S-1-5-21-1-2-3-500";
/// `FILE_ALL_ACCESS`, SDDL's `FA`.
const FA: u32 = 0x001f_01ff;

/// The guard's subjects: the pool user and `OWNER RIGHTS`.
fn guarded() -> Vec<Vec<u8>> {
    [POOL, OW].iter().map(|s| sid_bytes(&sid(s))).collect()
}

/// `acl`'s entries after the repair, on a folder or a file.
fn kept(aces: &[&[u8]], folder: bool) -> (bool, Vec<Vec<u8>>) {
    let acl = render(2, aces);
    let offends = guard_offends(&Acl::parse(&acl).unwrap(), folder, &guarded());
    let left = without_guarded(&acl, folder, &guarded()).unwrap();
    let left = Acl::parse(&left)
        .unwrap()
        .aces()
        .iter()
        .map(|a| a.to_vec())
        .collect();
    (offends, left)
}

/// `CPython`'s `tempfile.mkdtemp` (3.12.4 and later): `D:P(A;OICI;FA;;;OW)`. The owner's full
/// control, inherited by everything below, goes.
#[test]
fn pythons_temp_folder_loses_its_owner_rights_allow() {
    let cpython = ace(ALLOWED, OBJECT_INHERIT | CONTAINER_INHERIT, FA, OW);
    assert_eq!(kept(&[&cpython], true), (true, Vec::new()));
}

/// On a folder: an allow that lets the owner change permissions goes, and so does an execute
/// right its files inherit; traverse alone, the lawyer's entries and inherited ones stay.
#[test]
fn a_folder_keeps_traverse_and_entries_not_the_guards() {
    let modify_down = ace(
        ALLOWED,
        OBJECT_INHERIT | CONTAINER_INHERIT,
        0x0013_01bf,
        POOL,
    );
    let write_dac = ace(ALLOWED, 0, 0x0004_0000, OW);
    let generic_all = ace(ALLOWED, OBJECT_INHERIT | INHERIT_ONLY, 0x1000_0000, POOL);
    let traverse = ace(ALLOWED, CONTAINER_INHERIT, 0x0012_00a9, POOL);
    let lawyer = ace(ALLOWED, OBJECT_INHERIT | CONTAINER_INHERIT, FA, LAWYER);
    let inherited = ace(ALLOWED, INHERITED | OBJECT_INHERIT, FA, OW);
    let all: [&[u8]; 6] = [
        &modify_down,
        &write_dac,
        &generic_all,
        &traverse,
        &lawyer,
        &inherited,
    ];
    let want = vec![traverse.clone(), lawyer.clone(), inherited.clone()];
    assert_eq!(kept(&all, true), (true, want));
    let fine: [&[u8]; 3] = [&traverse, &lawyer, &inherited];
    assert!(!kept(&fine, true).0);
}

/// On a file: an execute allow goes unless the file's own explicit deny of execute comes first
/// (the Claude store's files, whose `Modify` is the plan's); a permission change goes always.
#[test]
fn a_file_keeps_an_execute_allow_only_behind_its_own_execute_deny() {
    let modify = ace(ALLOWED, 0, 0x0013_01bf, POOL);
    assert_eq!(kept(&[&modify], false), (true, Vec::new()));
    let deny = ace(DENIED, 0, 0x20, POOL);
    let both: [&[u8]; 2] = [&deny, &modify];
    assert_eq!(
        kept(&both, false),
        (false, vec![deny.clone(), modify.clone()])
    );
    let full = ace(ALLOWED, 0, FA, POOL);
    let with_full: [&[u8]; 2] = [&deny, &full];
    assert_eq!(kept(&with_full, false), (true, vec![deny.clone()]));
    // A deny that is only inherited by what is below doesn't count on the file.
    let below = ace(DENIED, INHERIT_ONLY | OBJECT_INHERIT, 0x20, POOL);
    let behind: [&[u8]; 2] = [&below, &modify];
    assert_eq!(kept(&behind, false), (true, vec![below.clone()]));
}
