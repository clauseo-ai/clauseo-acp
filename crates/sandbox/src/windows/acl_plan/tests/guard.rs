//! The guard on what the pool writes (`guard.rs`): off unless the policy asks.

use std::collections::BTreeSet;

use super::*;
use crate::windows::acl_bytes::{AceSpec, sid_bytes};

/// The guard's rights.
const GUARD: [Rights; 3] = [
    Rights::DenyExecute,
    Rights::DenyPermissionChange,
    Rights::OwnerModify,
];

/// The full policy with a session temp folder, and the switch as given.
fn policy(w: &World, on: bool) -> Policy {
    w.dir.dir("data/run/s1/tmp");
    w.full_builder()
        .tmpdir(w.at("data/run/s1/tmp"))
        .native_code_deny(on)
        .build()
        .unwrap()
}

/// The entries on `path`, as (rights, inherit, subject).
fn on(grants: &[Grant], path: &Path) -> Vec<(Rights, Inherit, &'static str)> {
    let on = grants.iter().filter(|g| g.path == path);
    on.map(|g| (g.rights, g.inherit, who(&g.sid))).collect()
}

/// Off (the default), the plan is today's: no guard entry, and the full plan's snapshot
/// (`the_full_plan`, unchanged) holds.
#[test]
fn off_by_default_the_plan_has_no_guard_entry() {
    let w = World::new("guard-off");
    assert!(!w.full_policy().native_code_deny());
    let off = grants_for(&w, &policy(&w, false), &sids());
    assert!(off.iter().all(|g| !GUARD.contains(&g.rights)), "{off:#?}");
    let mut with_matter = sids();
    with_matter.matter = Some(Sid::parse(MATTER_SID).unwrap());
    let off = grants_for(&w, &policy(&w, false), &with_matter);
    assert!(off.iter().all(|g| !GUARD.contains(&g.rights)));
}

/// On, the plan is today's with the guard's entries added, nothing else changed.
#[test]
fn on_the_plan_is_todays_plus_the_guard() {
    let w = World::new("guard-plus");
    let off = grants_for(&w, &policy(&w, false), &sids());
    let on = grants_for(&w, &policy(&w, true), &sids());
    let rest: Vec<&Grant> = on.iter().filter(|g| !GUARD.contains(&g.rights)).collect();
    assert_eq!(rest, off.iter().collect::<Vec<_>>());
}

#[test]
fn the_guarded_plan() {
    let w = World::new("guard-full");
    let grants = grants_for(&w, &policy(&w, true), &sids());
    insta::assert_snapshot!(render(&w, &grants));
}

/// Each place the plan lets the pool user add files gets the files-only execute deny and the
/// permission deny for the pool user, and `OWNER RIGHTS`' `Modify` below it; each file it may
/// write and run, an explicit execute deny; nothing else gets any.
#[test]
fn every_writable_place_and_only_those_gets_the_guard() {
    let w = World::new("guard-places");
    let grants = grants_for(&w, &policy(&w, true), &sids());
    let folders = ["matter", STATE, NOTES, "data/run/s1/tmp", STORE];
    for folder in folders {
        let path = w.canonical(folder);
        let guard: Vec<_> = on(&grants, &path)
            .into_iter()
            .filter(|(r, _, _)| GUARD.contains(r))
            .collect();
        let want = vec![
            (Rights::DenyExecute, Inherit::FilesBelow, "pool"),
            (Rights::DenyPermissionChange, Inherit::Below, "pool"),
            (Rights::OwnerModify, Inherit::Below, "owner-rights"),
        ];
        assert_eq!(guard, want, "{folder}");
    }
    let store_files = [".credentials.json", ".storage-write"];
    for name in store_files {
        let path = w.canonical(STORE).join(name);
        let deny = (Rights::DenyExecute, Inherit::ThisOnly, "pool");
        assert!(on(&grants, &path).contains(&deny), "{name}");
    }
    let guarded: BTreeSet<PathBuf> = grants
        .iter()
        .filter(|g| GUARD.contains(&g.rights))
        .map(|g| g.path.clone())
        .collect();
    let mut want: BTreeSet<PathBuf> = folders.iter().map(|f| w.canonical(f)).collect();
    want.extend(store_files.iter().map(|n| w.canonical(STORE).join(n)));
    // Not the runtime, read or allowed paths, the read-only names, or the credential link
    // (its allow has no execute right).
    assert_eq!(guarded, want);
}

/// With the matter SID as a second subject, each deny names it too; `OWNER RIGHTS` once.
#[test]
fn the_denies_name_every_subject_the_allows_name() {
    let w = World::new("guard-matter-sid");
    let mut subjects = sids();
    subjects.matter = Some(Sid::parse(MATTER_SID).unwrap());
    let grants = grants_for(&w, &policy(&w, true), &subjects);
    let guard: Vec<_> = on(&grants, &w.canonical("matter"))
        .into_iter()
        .filter(|(r, _, _)| GUARD.contains(r))
        .collect();
    let want = vec![
        (Rights::DenyExecute, Inherit::FilesBelow, "pool"),
        (Rights::DenyExecute, Inherit::FilesBelow, "matter"),
        (Rights::DenyPermissionChange, Inherit::Below, "pool"),
        (Rights::DenyPermissionChange, Inherit::Below, "matter"),
        (Rights::OwnerModify, Inherit::Below, "owner-rights"),
    ];
    assert_eq!(guard, want);
}

/// The entries as written on a folder: `(D;OIIO;0x20;;;pool)`, `(D;OICIIO;0xc0000;;;pool)` and
/// `(A;OICIIO;0x1301bf;;;OW)`, the last without `WRITE_DAC` or `WRITE_OWNER`.
#[test]
fn the_entries_carry_the_planned_flags_and_masks() {
    let w = World::new("guard-aces");
    let grants = grants_for(&w, &policy(&w, true), &sids());
    let matter = w.canonical("matter");
    let spec = |rights: Rights| {
        let grant = grants
            .iter()
            .find(|g| g.path == matter && g.rights == rights);
        AceSpec::for_grant(grant.unwrap(), true).unwrap()
    };
    let pool = sid_bytes(&Sid::parse(USER_SID).unwrap());
    let owner = sid_bytes(&Sid::parse(OWNER_RIGHTS_SID).unwrap());
    let ace = |kind, flags, mask, sid: &Vec<u8>| AceSpec {
        kind,
        flags,
        mask,
        sid: sid.clone(),
    };
    assert_eq!(spec(Rights::DenyExecute), ace(1, 0x9, 0x20, &pool));
    assert_eq!(
        spec(Rights::DenyPermissionChange),
        ace(1, 0xb, 0xc_0000, &pool)
    );
    assert_eq!(spec(Rights::OwnerModify), ace(0, 0xb, 0x0013_01bf, &owner));
    assert_eq!(Rights::OwnerModify.mask() & 0x000c_0000, 0);
    // On a file, a files-below entry is refused rather than written as something else.
    let deny = grants
        .iter()
        .find(|g| g.inherit == Inherit::FilesBelow)
        .unwrap();
    assert!(AceSpec::for_grant(deny, false).is_err());
}

/// `OWNER RIGHTS`' entry isn't a subject's allow, and names only `OWNER RIGHTS`.
#[test]
fn owner_rights_entry_is_not_a_subjects_allow() {
    assert!(!Rights::OwnerModify.is_allow());
    assert!(!Rights::OwnerModify.is_deny());
    assert!(Rights::DenyExecute.is_deny() && Rights::DenyPermissionChange.is_deny());
    assert_eq!(Rights::OwnerModify.fixed_subject(), Some(OWNER_RIGHTS_SID));
    assert_eq!(Rights::DenyExecute.fixed_subject(), None);
}
