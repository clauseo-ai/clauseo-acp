//! The guard over a fake registry: an existing value kept with the deny first, a second run
//! writing nothing, the absent value, an unparsable one and a Group Policy left degraded,
//! drift put back, and the removal taking off only the pool group's denies or putting the
//! backup back byte for byte. The lawyer's and every other SID's rights are evaluated before
//! and after.

use super::fake::FakeStore;
use super::sd::sid_bytes;
use super::sd::{is_deny, sid as entry_sid};
use super::test_sd::{
    A, D, GROUP, LAWYER, OTHER_GROUP, POOL_USER, ace, descriptor, sid, token, windows_11,
    windows_11_aces,
};
use super::{
    Backup, Change, DENY_MASK, Guard, LaunchRemoved, LaunchSd, Raw, STATE_VALUE, apply, remove,
};

/// The value's bytes; panics when it isn't binary.
fn bytes(store: &FakeStore) -> Vec<u8> {
    match &store.value {
        Raw::Binary(bytes) => bytes.clone(),
        other @ (Raw::Absent | Raw::OtherType) => panic!("not binary: {other:?}"),
    }
}

/// The value, parsed.
fn parsed(store: &FakeStore) -> LaunchSd {
    LaunchSd::parse(&bytes(store)).unwrap()
}

#[test]
fn an_existing_value_keeps_every_entry_exactly_with_the_deny_first() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change, applied.why),
        (Guard::Applied, Change::Added, None)
    );
    let value = parsed(&store);
    assert_eq!(value.aces().first(), Some(&ace(D, 0, DENY_MASK, GROUP)));
    assert_eq!(value.aces().get(1..), Some(windows_11_aces().as_slice()));
    // The backup holds the value as it was, and what was written, before the write.
    let backup = store.backup.clone().unwrap();
    assert_eq!(backup.original, Some(windows_11()));
    assert_eq!(backup.written, bytes(&store));
    assert_eq!(store.writes(), ["write_backup", "write", "write_state"]);
    assert_eq!(store.state.as_deref(), Some("applied"));
    assert_eq!(STATE_VALUE, "MachineLaunchDeny");
}

#[test]
fn a_second_run_changes_nothing() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    let after_first = store.clone();
    store.calls.clear();
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::Applied, Change::Unchanged)
    );
    assert_eq!(applied.line(), None);
    assert!(store.writes().is_empty(), "{:?}", store.calls);
    assert_eq!(store.value, after_first.value);
    assert_eq!(store.backup, after_first.backup);
}

#[test]
fn an_absent_value_gets_coms_default_plus_the_deny_and_its_removal_deletes_it() {
    let mut store = FakeStore::new(Raw::Absent);
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::Applied, Change::Added)
    );
    // COM's own launch descriptor (Windows 11's, in the fake), every entry kept, the deny first.
    let expected = LaunchSd::parse(&windows_11())
        .unwrap()
        .with_deny(&sid(GROUP))
        .unwrap();
    assert_eq!(parsed(&store), expected);
    assert_eq!(
        parsed(&store).aces().first(),
        Some(&ace(D, 0, DENY_MASK, GROUP))
    );
    assert_eq!(
        parsed(&store).aces().get(1..),
        Some(windows_11_aces().as_slice())
    );
    assert_eq!(
        store.backup.as_ref().and_then(|b| b.default.clone()),
        Some(windows_11())
    );
    assert_eq!(
        store.backup.as_ref().map(|b| b.original.clone()),
        Some(None)
    );
    // Idempotent from there too.
    store.calls.clear();
    assert_eq!(apply(&mut store, &sid(GROUP)).change, Change::Unchanged);
    assert_eq!(store.writes(), Vec::<&str>::new());
    // Uninstall: the value goes, and the backup and the state with it.
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    assert_eq!(store.value, Raw::Absent);
    assert_eq!((store.backup.clone(), store.state.clone()), (None, None));
    // A value written since but holding only the default plus ours is deleted too.
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    let rewritten = descriptor(expected.aces(), true);
    store.value = Raw::Binary(rewritten);
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    assert_eq!(store.value, Raw::Absent);
}

#[test]
fn an_unparsable_value_is_refused_and_left_degraded() {
    let null_dacl = {
        let mut bytes = windows_11();
        bytes[16..20].copy_from_slice(&[0; 4]);
        bytes
    };
    let no_dacl = {
        let mut bytes = windows_11();
        bytes[2] = 0;
        bytes
    };
    let old_form = descriptor(&[ace(A, 0, 1, "S-1-1-0")], false);
    let values = [
        Raw::Binary(vec![1, 2, 3]),
        Raw::Binary(null_dacl),
        Raw::Binary(no_dacl),
        Raw::Binary(old_form),
        Raw::OtherType,
    ];
    for value in values {
        let mut store = FakeStore::new(value.clone());
        let applied = apply(&mut store, &sid(GROUP));
        assert_eq!(applied.guard, Guard::Unparsable, "{value:?}");
        assert_eq!(applied.change, Change::Unchanged);
        assert!(applied.why.is_some());
        assert_eq!(store.value, value);
        assert_eq!(store.writes(), ["write_state"]);
        assert_eq!(store.state.as_deref(), Some("degraded: unparsable"));
        assert!(applied.line().unwrap().contains("degraded: unparsable"));
        // The removal leaves it as well, and still forgets the guard.
        assert!(matches!(
            remove(&mut store, &sid(GROUP)),
            Ok(LaunchRemoved::Left(_))
        ));
        assert_eq!(store.value, value);
        assert_eq!(store.state, None);
    }
}

#[test]
fn a_group_policy_value_leaves_the_guard_degraded_unless_it_denies_the_group() {
    // A policy without the deny: degraded, and the value still gets it for when the policy goes.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Ok(windows_11()));
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::PolicyOverrides, Change::Added)
    );
    assert!(parsed(&store).denies_first(&sid(GROUP)));
    assert_eq!(store.state.as_deref(), Some("degraded: group policy"));
    assert!(applied.guard.degraded());
    // A policy that won't convert is the same.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Err("not SDDL".to_owned()));
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::PolicyOverrides);
    // Firm IT's ACE in the policy: the guard holds through it.
    let mut aces = windows_11_aces();
    aces.insert(0, ace(D, 0, DENY_MASK, GROUP));
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Ok(descriptor(&aces, false)));
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::ByPolicy);
    assert!(!applied.guard.degraded());
    assert_eq!(store.state.as_deref(), Some("policy"));
    // The same ACE in SDDL with no owner or group: the strict parser refuses it, the policy
    // check doesn't need them and finds the deny.
    let mut no_owner = descriptor(&aces, false);
    no_owner[4..12].copy_from_slice(&[0; 8]);
    assert!(LaunchSd::parse(&no_owner).is_err());
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Ok(no_owner));
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::ByPolicy);
    // Our deny beside an entry in COM's old form: COM may reject the mixed list, so the deny
    // can't be counted on, and the guard is degraded.
    let mixed = descriptor(
        &[ace(D, 0, DENY_MASK, GROUP), ace(A, 0, 1, "S-1-1-0")],
        false,
    );
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.policy = Some(Ok(mixed));
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::PolicyOverrides);
    assert!(applied.why.unwrap().contains("old form"));
    // A lenient read still needs a DACL that holds the deny first.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    let mut no_dacl = descriptor(&aces, false);
    no_dacl[16..20].copy_from_slice(&[0; 4]);
    store.policy = Some(Ok(no_dacl));
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::PolicyOverrides);
    // An unreadable policy key: degraded, failed.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.fail = Some("policy");
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Failed);
    assert_eq!(store.state.as_deref(), Some("degraded: failed"));
}

#[test]
fn drift_is_put_back_and_the_first_backup_kept() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    // Something rewrote the value without our deny, and with one entry more.
    let mut aces = windows_11_aces();
    aces.push(ace(A, 0, 0xB, OTHER_GROUP));
    store.value = Raw::Binary(descriptor(&aces, false));
    store.calls.clear();
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::Applied, Change::PutBack)
    );
    assert!(applied.line().unwrap().contains("drift"));
    assert_eq!(parsed(&store).aces().get(1..), Some(aces.as_slice()));
    let backup = store.backup.clone().unwrap();
    assert_eq!(backup.original, Some(windows_11()));
    assert_eq!(backup.written, bytes(&store));
    assert!(backup.put_back);
    // The value is what Clauseo wrote last, but it holds someone else's edit: uninstall keeps
    // it and takes off only our deny, never the first original.
    assert_eq!(
        remove(&mut store, &sid(GROUP)),
        Ok(LaunchRemoved::DeniesRemoved(1))
    );
    assert_eq!(parsed(&store).aces(), aces.as_slice());
}

#[test]
fn after_a_put_back_on_an_absent_value_uninstall_keeps_the_others_entries() {
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    // Something rewrote the value: COM's default and one entry more, without our deny.
    let mut aces = windows_11_aces();
    aces.push(ace(A, 0, 0xB, OTHER_GROUP));
    store.value = Raw::Binary(descriptor(&aces, false));
    assert_eq!(apply(&mut store, &sid(GROUP)).change, Change::PutBack);
    assert_eq!(
        remove(&mut store, &sid(GROUP)),
        Ok(LaunchRemoved::DeniesRemoved(1))
    );
    assert_eq!(parsed(&store).aces(), aces.as_slice());
    // A put-back that left COM's default plus ours: what is left means the default, so the value
    // Clauseo made is deleted.
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    store.value = Raw::Binary(descriptor(&windows_11_aces(), true));
    apply(&mut store, &sid(GROUP));
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Deleted));
    // One that left less than the default (an entry taken off) is kept, only our deny off.
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    store.value = Raw::Binary(LaunchSd::windows_default().to_bytes());
    apply(&mut store, &sid(GROUP));
    assert_eq!(
        remove(&mut store, &sid(GROUP)),
        Ok(LaunchRemoved::DeniesRemoved(1))
    );
    assert_eq!(parsed(&store), LaunchSd::windows_default());
}

#[test]
fn a_failed_write_is_degraded_and_the_backup_comes_first() {
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.fail = Some("write");
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(
        (applied.guard, applied.change),
        (Guard::Failed, Change::Unchanged)
    );
    assert_eq!(store.value, Raw::Binary(windows_11()));
    assert!(store.backup.is_some());
    assert_eq!(store.state.as_deref(), Some("degraded: failed"));
    // A failed backup writes nothing to the value.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.fail = Some("write_backup");
    assert_eq!(apply(&mut store, &sid(GROUP)).guard, Guard::Failed);
    assert!(!store.calls.contains(&"write"));
    // A state that can't be written is said, and the guard is still what it is.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.fail = Some("write_state");
    let applied = apply(&mut store, &sid(GROUP));
    assert_eq!(applied.guard, Guard::Applied);
    assert!(applied.why.unwrap().contains("state wasn't recorded"));
}

#[test]
fn uninstall_restores_byte_for_byte_when_untouched_and_otherwise_takes_off_only_ours() {
    // Untouched since: the original bytes come back, even in a layout ours doesn't write.
    let original = descriptor(&windows_11_aces(), true);
    let mut store = FakeStore::new(Raw::Binary(original.clone()));
    apply(&mut store, &sid(GROUP));
    assert_ne!(bytes(&store), original);
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Restored));
    assert_eq!(bytes(&store), original);
    assert_eq!((store.backup.clone(), store.state.clone()), (None, None));
    // Changed since (an entry added after ours): only the group's deny goes.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    let mut aces = parsed(&store).aces().to_vec();
    aces.push(ace(A, 0, 0xB, OTHER_GROUP));
    store.value = Raw::Binary(descriptor(&aces, false));
    assert_eq!(
        remove(&mut store, &sid(GROUP)),
        Ok(LaunchRemoved::DeniesRemoved(1))
    );
    let mut expected = windows_11_aces();
    expected.push(ace(A, 0, 0xB, OTHER_GROUP));
    assert_eq!(parsed(&store).aces(), expected.as_slice());
    // A second removal finds nothing to do.
    assert_eq!(remove(&mut store, &sid(GROUP)), Ok(LaunchRemoved::Nothing));
    // A failed write keeps the backup for the next run.
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    apply(&mut store, &sid(GROUP));
    store.fail = Some("write");
    assert!(remove(&mut store, &sid(GROUP)).is_err());
    assert!(store.backup.is_some());
}

#[test]
fn the_pool_is_denied_and_every_other_sids_rights_are_unchanged() {
    let before = LaunchSd::parse(&windows_11()).unwrap();
    let mut store = FakeStore::new(Raw::Binary(windows_11()));
    store.backup = None::<Backup>;
    apply(&mut store, &sid(GROUP));
    let after = parsed(&store);
    let pool = token(POOL_USER, &[GROUP]);
    assert_eq!(before.granted(&pool), 0xF);
    assert_eq!(
        after.granted(&pool),
        0,
        "no launch or activation, local or remote"
    );
    let others = [
        token(LAWYER, &[]),
        token(LAWYER, &["S-1-5-32-544"]),
        token(LAWYER, &[OTHER_GROUP, "S-1-5-32-562"]),
        vec![sid("S-1-5-18")],
        vec![sid("S-1-15-2-1"), sid("S-1-1-0")],
    ];
    for others in others {
        assert_eq!(
            after.granted(&others),
            before.granted(&others),
            "{others:?}"
        );
    }
    // Structurally: the one entry added is a deny naming the group.
    let group = sid_bytes(&sid(GROUP));
    let added: Vec<&Vec<u8>> = after
        .aces()
        .iter()
        .filter(|entry| !before.aces().contains(entry))
        .collect();
    assert_eq!(added.len(), 1);
    assert!(
        added
            .iter()
            .all(|e| is_deny(e) && entry_sid(e) == Some(group.as_slice()))
    );
    // The absent case: COM's default's grants stay for everyone else too.
    let mut store = FakeStore::new(Raw::Absent);
    apply(&mut store, &sid(GROUP));
    let default = LaunchSd::parse(&windows_11()).unwrap();
    assert_eq!(parsed(&store).granted(&pool), 0);
    for others in [token(LAWYER, &[]), token(LAWYER, &["S-1-5-32-562"])] {
        assert_eq!(parsed(&store).granted(&others), default.granted(&others));
    }
}
