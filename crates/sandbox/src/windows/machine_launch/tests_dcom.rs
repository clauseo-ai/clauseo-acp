//! DCOM's rewrites (a VM run): within a millisecond of a write, DCOM merges Windows' default
//! entries into the launch value, and re-serialises both values (here: allows reordered, padding
//! after every entry, the DACL laid out first). The removal finds Clauseo's values again by
//! meaning, takes off only the deny Clauseo added, keeps one that was there before, and deletes
//! a value it made only when what is left means COM's default. With no value, COM's own launch
//! descriptor is the base.

use super::fake::{FakeStore, dcom_rewrite, machine_access, pad};
use super::test_sd::{A, D, GROUP, OTHER_GROUP, ace, descriptor, sid, windows_11, windows_11_aces};
use super::{DENY_MASK, Guard, LaunchRemoved, LaunchSd, Meaning, Raw, apply, remove};

/// A machine with no launch value and no access value.
fn bare() -> FakeStore {
    let mut store = FakeStore::new(Raw::Absent);
    store.access = Raw::Absent;
    store
}

/// The value's bytes; panics when it isn't binary.
fn bytes(raw: &Raw) -> Vec<u8> {
    match raw {
        Raw::Binary(bytes) => bytes.clone(),
        other @ (Raw::Absent | Raw::OtherType) => panic!("not binary: {other:?}"),
    }
}

#[test]
fn meaning_ignores_order_layout_and_padding_but_not_entries_or_owner() {
    let mut aces = windows_11_aces();
    let base = descriptor(&aces, false);
    aces.reverse();
    let padded: Vec<Vec<u8>> = aces.iter().map(|entry| pad(entry)).collect();
    assert!(Meaning::same(&base, &descriptor(&padded, true)));
    assert_ne!(base, descriptor(&padded, true));
    // Another mask, one entry more or fewer: another meaning.
    let mut other = windows_11_aces();
    other[1] = ace(A, 0, 0xB, "S-1-1-0");
    assert!(!Meaning::same(&base, &descriptor(&other, false)));
    let mut more = windows_11_aces();
    more.push(ace(A, 0, 0xB, OTHER_GROUP));
    assert!(!Meaning::same(&base, &descriptor(&more, false)));
    // Another owner (the bytes at the owner's SID: Administrators' last sub-authority).
    let mut owner = base.clone();
    let at = usize::try_from(u32::from_le_bytes(owner[4..8].try_into().unwrap())).unwrap();
    owner[at + 12] ^= 1;
    assert!(!Meaning::same(&base, &owner));
    assert!(!Meaning::same(&base, &[1, 2, 3]));
}

#[test]
fn an_absent_value_is_coms_default_plus_the_deny_first_and_unread_is_degraded() {
    let mut store = bare();
    apply(&mut store, &sid(GROUP));
    let written = LaunchSd::parse(&bytes(&store.value)).unwrap();
    assert_eq!(written.aces().first(), Some(&ace(D, 0, DENY_MASK, GROUP)));
    for entry in windows_11_aces() {
        assert!(written.aces().contains(&entry), "{entry:?}");
    }
    assert_eq!(written.aces().len(), windows_11_aces().len() + 1);
    // COM's launch descriptor can't be read: nothing written, degraded.
    let mut store = bare();
    store.effective_launch = Err("CoGetSystemSecurityPermissions failed (5)".to_owned());
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::LaunchUnknown);
    assert_eq!(store.writes(), ["write_state"]);
    assert_eq!(store.value, Raw::Absent);
    assert_eq!(
        store.state.as_deref(),
        Some("degraded: machine launch unknown")
    );
}

#[test]
fn values_dcom_rewrites_at_once_are_still_removed() {
    // No values: DCOM reorders and pads both; what it made counts as Clauseo's.
    let mut store = bare();
    store.rewrites = true;
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Applied);
    let backup = store.backup.clone().unwrap();
    assert_eq!(backup.written, bytes(&store.value));
    assert_ne!(backup.history.first(), Some(&backup.written));
    assert_ne!(bytes(&store.access), machine_access());
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    // A value without every default entry: DCOM merges them in, and the original still goes
    // back byte for byte.
    let original = LaunchSd::windows_default().to_bytes();
    let mut store = FakeStore::new(Raw::Binary(original.clone()));
    store.rewrites = true;
    apply(&mut store, &sid(GROUP));
    let merged = LaunchSd::parse(&bytes(&store.value)).unwrap();
    assert!(merged.aces().len() > LaunchSd::windows_default().aces().len() + 1);
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(store.value, Raw::Binary(original));
}

#[test]
fn values_dcom_rewrites_later_still_read_as_clauseos() {
    let mut store = bare();
    apply(&mut store, &sid(GROUP));
    store.value = Raw::Binary(dcom_rewrite(&bytes(&store.value), &windows_11_aces()));
    store.access = Raw::Binary(dcom_rewrite(&bytes(&store.access), &[]));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    // A value moved to `…Old` and padded there is still Clauseo's.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.moves = true;
    apply(&mut store, &sid(GROUP));
    store.old = Raw::Binary(dcom_rewrite(&bytes(&store.old), &[]));
    remove(&mut store, &sid(GROUP)).unwrap();
    assert_eq!(store.old, Raw::Absent);
}

#[test]
fn an_identical_deny_there_before_is_kept() {
    // IT's deny, the same by meaning (padded), after the allows: ours goes in front.
    let mut aces = windows_11_aces();
    aces.push(pad(&ace(D, 0, DENY_MASK, GROUP)));
    let original = descriptor(&aces, false);
    let mut store = FakeStore::new(Raw::Binary(original.clone()));
    apply(&mut store, &sid(GROUP));
    assert!(store.backup.as_ref().unwrap().deny_existed);
    // Untouched since: the original, IT's deny in it, goes back.
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(store.value, Raw::Binary(original.clone()));
    // Changed since: no deny comes off.
    let mut store = FakeStore::new(Raw::Binary(original));
    apply(&mut store, &sid(GROUP));
    let mut changed = LaunchSd::parse(&bytes(&store.value))
        .unwrap()
        .aces()
        .to_vec();
    changed.push(ace(A, 0, 0xB, OTHER_GROUP));
    store.value = Raw::Binary(descriptor(&changed, false));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Nothing));
    assert_eq!(store.value, Raw::Binary(descriptor(&changed, false)));
    // IT's deny first already: Clauseo writes nothing and removes nothing.
    let mut first = vec![ace(D, 0, DENY_MASK, GROUP)];
    first.extend(windows_11_aces());
    let mut store = FakeStore::new(Raw::Binary(descriptor(&first, false)));
    apply(&mut store, &sid(GROUP));
    assert_eq!(store.backup, None);
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Nothing));
    assert_eq!(store.value, Raw::Binary(descriptor(&first, false)));
}

#[test]
fn a_group_policy_launch_value_with_no_local_value_copies_nothing() {
    // The policy without the deny: nothing written, degraded.
    let mut store = bare();
    store.policy = Some(Ok(windows_11()));
    store.effective_launch = Ok(windows_11());
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::PolicyOverrides);
    assert!(
        applied
            .why
            .unwrap()
            .contains("Group Policy MachineLaunchRestriction is set")
    );
    assert!(!store.calls.contains(&"effective_launch"));
    assert_eq!(store.writes(), ["write_state"]);
    assert_eq!(
        (store.value.clone(), store.access.clone()),
        (Raw::Absent, Raw::Absent)
    );
    assert_eq!(store.backup, None);
    assert_eq!(store.state.as_deref(), Some("degraded: group policy"));
    // A policy that can't be read: nothing copied either.
    let mut store = bare();
    store.fail = Some("policy");
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Failed);
    assert_eq!(store.writes(), ["write_state"]);
    // A policy holding the deny: still nothing copied, and the guard holds through the policy.
    let mut first = vec![ace(D, 0, DENY_MASK, GROUP)];
    first.extend(windows_11_aces());
    let mut store = bare();
    store.policy = Some(Ok(descriptor(&first, false)));
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::ByPolicy);
    assert_eq!(store.value, Raw::Absent);
    // A local value there: it still gets the deny, for when the policy goes.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Ok(windows_11()));
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::PolicyOverrides);
    let value = LaunchSd::parse(&bytes(&store.value)).unwrap();
    assert!(value.denies_first(&sid(GROUP)));
}
