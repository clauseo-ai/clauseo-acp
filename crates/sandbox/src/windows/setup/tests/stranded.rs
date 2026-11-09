//! Pool accounts the record doesn't list (`stranded.rs`): set-up adopts the proven ones back
//! when the record's pool was blanked or corrupted, and `--remove` removes them, exiting 0; a
//! lost or untrusted record has them removed, never adopted; an account one condition short
//! (in the group without the mark, or marked outside the group) is named and left; a
//! `ClauseoSb01` a user made is ignored.

use std::collections::BTreeSet;

use super::fake::{Account, OWNER_NAME};
use super::remove::pool;
use super::*;
use crate::windows::pool::POOL_PREFIX;
use crate::windows::setup::record_values::{RawRecord, decode_untrusting_pool};

/// The eight users a fresh set-up made: `ClauseoSb01` to `ClauseoSb08`, 1001 to 1008.
fn eight() -> Vec<Sid> {
    (1001..=1008).map(sid).collect()
}

/// A fresh pool whose record then lost its `PoolSids` (blanked, or malformed and read as empty).
fn blanked(test: &str) -> (FakeMachine, FakeAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.record.as_mut().unwrap().pool.clear();
    (machine, acl)
}

/// An account outside set-up: `hidden` and in `group` as asked.
fn account(machine: &mut FakeMachine, name: &str, rid: u32, group: Option<Sid>, hidden: bool) {
    machine.state.accounts.insert(
        name.to_owned(),
        Account {
            sid: sid(rid),
            password: "someone's".to_owned(),
            groups: group.into_iter().collect(),
            hidden,
        },
    );
}

/// The pool users' names left on the machine.
fn pool_names(machine: &FakeMachine) -> Vec<String> {
    let names = machine.state.accounts.keys();
    names
        .filter(|name| name.starts_with(POOL_PREFIX))
        .cloned()
        .collect()
}

/// Whether `left` names `name`.
fn names(left: &[String], name: &str) -> bool {
    left.iter().any(|line| line.starts_with(name))
}

#[test]
fn set_up_adopts_a_blanked_records_eight_users_and_makes_none() {
    let (mut machine, mut acl) = blanked("stranded-adopt");
    let passwords: Vec<String> = (1..=8)
        .map(|n| {
            machine.state.accounts[&format!("ClauseoSb0{n}")]
                .password
                .clone()
        })
        .collect();
    let from = machine.calls.len();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert!(report.made.is_empty(), "{:?}", report.made);
    let calls = machine.calls_since(from);
    assert!(
        !calls.iter().any(|c| c.starts_with("create_user")),
        "{calls:?}"
    );
    for n in 1..=8 {
        assert!(calls.contains(&format!("set_password ClauseoSb0{n}").as_str()));
    }
    // Same SIDs, so the ledger's and the ACLs' references stay good; new passwords.
    assert_eq!(machine.state.record.as_ref().unwrap().pool, eight());
    let after: Vec<String> = (1..=8)
        .map(|n| {
            machine.state.accounts[&format!("ClauseoSb0{n}")]
                .password
                .clone()
        })
        .collect();
    assert!(after.iter().zip(&passwords).all(|(new, old)| new != old));
    assert_eq!(report.reset.len(), 8);
}

#[test]
fn set_up_adopts_the_users_a_malformed_pool_sids_lost() {
    // A malformed PoolSids reads as an empty pool, never as a shorter list.
    let raw = RawRecord {
        owner: Some(OWNER.to_owned()),
        group: Some(sid(1000).as_str().to_owned()),
        pool: Some(
            "S-1-5-21-5-5-5-1001\0not a sid\0\0"
                .encode_utf16()
                .collect(),
        ),
        data_dir: Some("C:\\data".to_owned()),
        removing: None,
    };
    let record = decode_untrusting_pool(raw).unwrap().unwrap();
    assert!(record.pool.is_empty(), "{:?}", record.pool);
    assert_eq!(record.group, sid(1000));
    let (mut machine, mut acl) = blanked("stranded-malformed");
    let report = set_up_fresh(&mut machine, &mut acl);
    assert!(report.made.is_empty(), "{:?}", report.made);
    assert_eq!(machine.state.record.unwrap().pool, eight());
}

#[test]
fn remove_takes_a_blanked_records_users_and_exits_0() {
    let (mut machine, mut acl) = pool("stranded-remove", Some(owner()));
    machine.m.state.record.as_mut().unwrap().pool.clear();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(
        (report.accounts, report.profiles, report.group),
        (8, 8, true)
    );
    assert!(report.left.is_empty(), "{:?}", report.left);
    assert_eq!(pool_names(&machine.m), Vec::<String>::new());
    assert_eq!(machine.m.state.group, None);
    assert_eq!(machine.m.state.record, None);
    assert!(machine.m.state.remote_denied.is_empty());
    // The record listed them, with Removing, before anything went.
    assert!(
        machine
            .lines()
            .contains(&"write_record removing".to_owned())
    );
}

#[test]
fn a_corrupt_record_has_set_up_remove_its_users_and_make_a_new_pool() {
    let (mut machine, mut acl) = blanked("stranded-corrupt-setup");
    machine.corrupt = Some("the record has no OwnerSid");
    let from = machine.calls.len();
    let report = set_up_fresh(&mut machine, &mut acl);
    let calls = machine.calls_since(from);
    for n in 1..=8 {
        let user = format!("delete_user ClauseoSb0{n}");
        assert!(calls.contains(&user.as_str()), "{calls:?}");
    }
    assert!(calls.contains(&"delete_group 1000"));
    // Nothing adopted (D5): new accounts, new SIDs, a new group.
    assert_eq!(report.made.len(), 8);
    let record = machine.state.record.unwrap();
    assert!(record.pool.iter().all(|sid| !eight().contains(sid)));
    assert_ne!(record.group, sid(1000));
}

#[test]
fn a_corrupt_record_has_remove_take_its_users_and_exit_0() {
    let (mut machine, mut acl) = pool("stranded-corrupt-remove", Some(owner()));
    machine.m.corrupt = Some("the record's GroupSid is malformed");
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(
        (report.accounts, report.profiles, report.group),
        (8, 8, true)
    );
    assert_eq!(pool_names(&machine.m), Vec::<String>::new());
    assert_eq!(machine.m.state.group, None);
    assert!(machine.lines().contains(&"delete_record".to_owned()));
    assert_eq!(machine.m.corrupt, None);
}

#[test]
fn a_member_without_the_mark_is_never_adopted_or_removed_but_named() {
    let (mut machine, mut acl) = blanked("stranded-unmarked");
    account(&mut machine, "ClauseoSb09", 555, Some(sid(1000)), true);
    machine.unflagged.insert("ClauseoSb09".to_owned());
    let report = set_up_fresh(&mut machine, &mut acl);
    let record = machine.state.record.clone().unwrap();
    assert!(!record.pool.contains(&sid(555)));
    assert!(report.pool.iter().all(|user| user.sid != sid(555)));
    assert_eq!(machine.state.accounts["ClauseoSb09"].password, "someone's");

    let mut remover = super::remove_fake::Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    remover.m.state.record.as_mut().unwrap().pool.clear();
    let removed = remove(&mut remover, &mut acl, false).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(report.accounts, 8);
    assert!(names(&report.left, "ClauseoSb09"), "{:?}", report.left);
    assert!(names(&report.left, "the group ClauseoSandboxUsers"));
    assert_eq!(pool_names(&remover.m), ["ClauseoSb09"]);
    assert_eq!(remover.m.state.group, Some(sid(1000)));
}

#[test]
fn a_marked_account_outside_the_group_is_never_adopted_or_removed_but_named() {
    let (mut machine, mut acl) = blanked("stranded-outside");
    account(&mut machine, "ClauseoSb10", 556, None, true);
    set_up_fresh(&mut machine, &mut acl);
    assert!(
        !machine
            .state
            .record
            .clone()
            .unwrap()
            .pool
            .contains(&sid(556))
    );
    assert_eq!(machine.state.accounts["ClauseoSb10"].password, "someone's");

    let mut remover = super::remove_fake::Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    remover.m.state.record.as_mut().unwrap().pool.clear();
    let removed = remove(&mut remover, &mut acl, false).unwrap();
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert!(names(&report.left, "ClauseoSb10"), "{:?}", report.left);
    assert!(report.group);
    assert_eq!(pool_names(&remover.m), ["ClauseoSb10"]);
}

#[test]
fn a_clauseo_sb01_a_user_made_is_ignored_by_set_up_and_remove() {
    let mut machine = FakeMachine::new("stranded-squat");
    account(&mut machine, "ClauseoSb01", 557, None, false);
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert!(report.pool.iter().all(|user| user.sid != sid(557)));
    assert!(
        !machine
            .state
            .record
            .clone()
            .unwrap()
            .pool
            .contains(&sid(557))
    );

    let mut remover = super::remove_fake::Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(FakeAcl::default());
    remover.m.state.record.as_mut().unwrap().pool.clear();
    let Removed::All(report) = remove(&mut remover, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert!(report.left.is_empty(), "{:?}", report.left);
    assert_eq!(pool_names(&remover.m), ["ClauseoSb01"]);
    let squat = &remover.m.state.accounts["ClauseoSb01"];
    assert_eq!(
        (squat.password.as_str(), squat.groups.len()),
        ("someone's", 0)
    );
}

#[test]
fn no_record_has_a_squatted_unmarked_member_named_and_left_with_its_group() {
    let (mut machine, _acl) = pool("stranded-no-record", Some(owner()));
    machine.m.state.record = None;
    account(&mut machine.m, "Mallory", 558, Some(sid(1000)), false);
    let mut acl = machine.acl(FakeAcl::default());
    let Removed::All(report) = remove(&mut machine, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!(report.accounts, 8);
    assert!(names(&report.left, "Mallory"), "{:?}", report.left);
    assert_eq!(pool_names(&machine.m), Vec::<String>::new());
    assert!(machine.m.state.accounts.contains_key("Mallory"));
    assert_eq!(machine.m.state.group, Some(sid(1000)));
    assert!(!machine.lines().contains(&"delete_group 1000".to_owned()));
}

#[test]
fn no_record_has_set_up_remove_the_old_pool_then_refuse_while_a_stranger_holds_the_group() {
    let mut machine = FakeMachine::new("stranded-no-record-setup");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.record = None;
    account(&mut machine, "Mallory", 558, Some(sid(1000)), false);
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Group);
    assert!(failure.cause.contains("Mallory"), "{}", failure.cause);
    assert_eq!(pool_names(&machine), Vec::<String>::new());
    assert!(machine.state.accounts.contains_key("Mallory"));
    assert!(machine.state.accounts.contains_key(OWNER_NAME));
    // Taken out of the group, the stranger no longer holds it: the next run starts afresh.
    machine.state.accounts.remove("Mallory");
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.made.len(), 8);
    assert!(report.group_made);
    let unused: BTreeSet<Sid> = eight().into_iter().collect();
    assert!(report.pool.iter().all(|user| !unused.contains(&user.sid)));
}
