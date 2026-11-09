//! More of `stranded.rs` against the fakes: the comment and `MarkerVersion`, a domain group of
//! the group's name, the clean-up of each removed account (disabled first, its rights, its
//! tasks), a corrupt record's clean-up around its pool and its owner's say (D-B9), the
//! refusal's names, and the failures: a delete that fails, a revoke that fails.

use super::fake::Account;
use super::remove::pool;
use super::remove_fake::Remover;
use super::*;
use crate::windows::pool::POOL_PREFIX;
use crate::windows::setup::record_values::CopyOwner;
use crate::windows::setup::stranded::{MARKER_VERSION, PartialRecord};

/// The eight users a fresh set-up made: `ClauseoSb01` to `ClauseoSb08`, 1001 to 1008.
fn eight() -> Vec<Sid> {
    (1001..=8 + 1000).map(sid).collect()
}

/// A fresh pool whose record then lost its `PoolSids`.
fn blanked(test: &str) -> (FakeMachine, FakeAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.record.as_mut().unwrap().pool.clear();
    (machine, acl)
}

/// An account outside set-up, hidden (the older mark's half), in `group` as asked.
fn account(machine: &mut FakeMachine, name: &str, rid: u32, group: Option<Sid>) {
    machine.state.accounts.insert(
        name.to_owned(),
        Account {
            sid: sid(rid),
            password: "someone's".to_owned(),
            groups: group.into_iter().collect(),
            hidden: true,
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

/// [`pool`] with its record gone: what `stranded::sweep` takes.
fn no_record(test: &str) -> (Remover, super::remove_fake::RemoveAcl) {
    let (mut machine, acl) = pool(test, Some(owner()));
    machine.m.state.record = None;
    (machine, acl)
}

/// [`pool`] with a corrupt record that still reads as `partial`.
fn corrupt(test: &str, partial: PartialRecord) -> (Remover, super::remove_fake::RemoveAcl) {
    let (mut machine, acl) = pool(test, Some(owner()));
    machine.m.stranded.corrupt = Some("the record's DataDir is missing");
    machine.m.stranded.partial = partial;
    (machine, acl)
}

/// Where `line` is in `lines`.
fn at(lines: &[String], line: &str) -> usize {
    lines
        .iter()
        .position(|each| each == line)
        .unwrap_or_else(|| panic!("no {line} in {lines:?}"))
}

#[test]
fn set_up_comments_every_user_and_writes_marker_version_2() {
    let mut machine = FakeMachine::new("stranded-comment");
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(machine.stranded.marker_version, Some(MARKER_VERSION));
    for n in 1..=8 {
        assert!(
            machine
                .stranded
                .commented
                .contains(&format!("ClauseoSb0{n}"))
        );
    }
}

#[test]
fn a_pool_from_before_the_comment_is_adopted_with_new_profiles_and_marked() {
    let (mut machine, mut acl) = blanked("stranded-older-pool");
    machine.stranded.commented.clear();
    machine.stranded.marker_version = None;
    let from = machine.calls.len();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert_eq!(report.made.len(), 0, "{:?}", report.made);
    assert_eq!(machine.state.record.clone().unwrap().pool, eight());
    // Their history is unknown: each profile is made again.
    assert_eq!(report.profiles_reset.as_ref().map(Vec::len), Ok(8));
    let calls = machine.calls_since(from);
    assert!(calls.contains(&"delete_profile 1001"), "{calls:?}");
    for n in 1..=8 {
        assert!(
            machine
                .stranded
                .commented
                .contains(&format!("ClauseoSb0{n}"))
        );
    }
    assert_eq!(machine.stranded.marker_version, Some(MARKER_VERSION));
}

#[test]
fn once_marked_a_pool_never_adopts_an_account_with_the_older_mark_alone() {
    let (mut machine, mut acl) = blanked("stranded-older-mark");
    account(&mut machine, "ClauseoSb09", 559, Some(sid(1000)));
    let report = set_up_fresh(&mut machine, &mut acl);
    assert!(report.pool.iter().all(|user| user.sid != sid(559)));
    assert_eq!(machine.state.accounts["ClauseoSb09"].password, "someone's");
    // With no record either, and MarkerVersion gone with it, a commented member still bars it.
    machine.state.record = None;
    machine.stranded.marker_version = None;
    let mut remover = Remover::new(machine, Some(owner()));
    let mut acl = remover.acl(acl);
    let Removed::All(removed) = remove(&mut remover, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!(removed.accounts, 8);
    assert!(
        removed
            .left
            .iter()
            .any(|line| line.starts_with("ClauseoSb09"))
    );
    assert_eq!(pool_names(&remover.m), ["ClauseoSb09"]);
}

#[test]
fn a_domain_group_named_clauseo_sandbox_users_is_never_used() {
    let (mut machine, mut acl) = no_record("stranded-domain-group");
    let domain_group = Sid::parse("S-1-5-21-9-9-9-1000").unwrap();
    machine.m.state.group = Some(domain_group.clone());
    for account in machine.m.state.accounts.values_mut() {
        if account.groups.remove(&sid(1000)) {
            account.groups.insert(domain_group.clone());
        }
    }
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(removed, Removed::NothingSetUp);
    assert_eq!(pool_names(&machine.m).len(), 8);
    assert_eq!(machine.lines(), ["read_record", "lock 0", "unlock"]);
}

#[test]
fn a_removal_revokes_each_users_rights_and_deletes_their_tasks() {
    for test in ["stranded-rights-recorded", "stranded-rights-swept"] {
        let (mut machine, mut acl) = pool(test, Some(owner()));
        if test.ends_with("swept") {
            machine.m.state.record = None;
        }
        machine.m.stranded.rights = eight().into_iter().collect();
        machine.m.stranded.tasks = eight().into_iter().collect();
        assert!(matches!(
            remove(&mut machine, &mut acl, false),
            Ok(Removed::All(_))
        ));
        assert!(machine.m.stranded.rights.is_empty(), "{test}");
        assert!(machine.m.stranded.tasks.is_empty(), "{test}");
        let lines = machine.lines();
        assert!(at(&lines, "delete_user ClauseoSb01") < at(&lines, "revoke_rights 1001"));
    }
}

#[test]
fn a_stranded_user_whose_delete_fails_is_left_disabled_and_the_removal_exits_4() {
    let (mut machine, mut acl) = no_record("stranded-delete-fails");
    machine.m.fail("delete_user ClauseoSb03");
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(
        failure.cause.contains("ClauseoSb03 (disabled)"),
        "{}",
        failure.cause
    );
    assert!(machine.m.state.accounts.contains_key("ClauseoSb03"));
    assert!(machine.m.stranded.disabled.contains("ClauseoSb03"));
    let lines = machine.lines();
    assert!(at(&lines, "disable ClauseoSb03") < at(&lines, "delete_user ClauseoSb03"));
    // The next run finds it again by the same proof, and finishes.
    let Removed::All(report) = remove(&mut machine, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!((report.accounts, report.group), (1, true));
    assert_eq!(pool_names(&machine.m), Vec::<String>::new());
}

#[test]
fn set_up_disables_a_stranded_user_before_a_delete_that_fails() {
    let mut machine = FakeMachine::new("stranded-setup-delete-fails");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.record = None;
    machine.fail("delete_user ClauseoSb02");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert!(machine.state.accounts.contains_key("ClauseoSb02"));
    assert!(machine.stranded.disabled.contains("ClauseoSb02"));
    assert!(failure.cause.contains("injected"), "{}", failure.cause);
}

#[test]
fn a_revoke_that_fails_is_named_and_the_removal_goes_on() {
    let (mut machine, mut acl) = no_record("stranded-revoke-fails");
    machine.m.fail("revoke_rights 1001");
    let Removed::All(report) = remove(&mut machine, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!(report.accounts, 8);
    let named = report
        .left
        .iter()
        .any(|l| l.starts_with("ClauseoSb01's user rights"));
    assert!(named, "{:?}", report.left);
}

#[test]
fn a_corrupt_record_has_its_network_denies_ledger_markers_and_tasks_go_before_its_values() {
    let (mut machine, mut acl) = corrupt("stranded-corrupt-around", PartialRecord::default());
    machine.m.stranded.partial = PartialRecord {
        owner: Some(owner()),
        group: Some(sid(1000)),
        data_dir: Some(machine.m.data_dir()),
    };
    machine.m.state.copy_owner = Some(CopyOwner::Machine);
    let Removed::All(report) = remove(&mut machine, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!((report.accounts, report.group), (8, true));
    let lines = machine.lines();
    let record = at(&lines, "delete_record");
    for line in [
        "remove_network",
        "remove ProgramData/Microsoft/DeviceSync 1000",
        "owners_ledger",
        "clear_markers",
        "unregister_stop_task",
    ] {
        assert!(at(&lines, line) < record, "{line} in {lines:?}");
    }
    assert!(at(&lines, "delete_user ClauseoSb08") < at(&lines, "remove_network"));
}

#[test]
fn a_corrupt_record_with_no_owner_still_takes_the_network_off_but_leaves_the_ledger() {
    let partial = PartialRecord {
        owner: None,
        group: Some(sid(1000)),
        data_dir: None,
    };
    let (mut machine, mut acl) = corrupt("stranded-corrupt-no-owner", partial);
    assert!(matches!(
        remove(&mut machine, &mut acl, true),
        Ok(Removed::All(_))
    ));
    let lines = machine.lines();
    assert!(lines.contains(&"remove_network".to_owned()));
    assert!(!lines.contains(&"owners_ledger".to_owned()));
    assert!(!lines.contains(&"clear_markers".to_owned()));
    assert_eq!(machine.m.state.record, None);
}

#[test]
fn a_corrupt_record_whose_owner_reads_is_theirs_alone_to_remove_per_user() {
    let partial = PartialRecord {
        owner: Some(owner()),
        ..PartialRecord::default()
    };
    let (machine, _) = corrupt("stranded-corrupt-d-b9", partial);
    let other = Sid::parse(super::fake::OTHER_OWNER).unwrap();
    let mut machine = Remover::new(machine.m, Some(other));
    let mut acl = machine.acl(FakeAcl::default());
    let state = machine.m.state.clone();
    assert_eq!(
        remove(&mut machine, &mut acl, false).unwrap(),
        Removed::NotOwner
    );
    assert_eq!(machine.m.state, state);
    assert_eq!(machine.lines(), ["read_record"]);
}

#[test]
fn set_ups_refusal_names_exactly_the_groups_unprovable_members() {
    let mut machine = FakeMachine::new("stranded-refusal-names");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.state.record = None;
    // In the group: unmarked, and a domain account. Outside it: one with the older mark.
    account(&mut machine, "Mallory", 558, Some(sid(1000)));
    machine.stranded.unflagged.insert("Mallory".to_owned());
    machine.stranded.foreign.push("CORP\\eve".to_owned());
    account(&mut machine, "ClauseoSb10", 560, None);
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Group);
    assert!(
        failure.cause.contains("(Mallory, CORP\\eve)"),
        "{}",
        failure.cause
    );
    assert!(!failure.cause.contains("ClauseoSb10"), "{}", failure.cause);
}

#[test]
fn re_running_set_up_twice_on_an_all_free_pool_keeps_the_same_eight_users_with_either_mark() {
    for older in [false, true] {
        let mut machine = FakeMachine::new("stranded-rerun-fixed");
        let mut acl = FakeAcl::default();
        set_up_fresh(&mut machine, &mut acl);
        if older {
            machine.stranded.commented.clear();
            machine.stranded.marker_version = None;
        }
        let users = |machine: &FakeMachine| -> Vec<(String, Sid)> {
            let names = pool_names(machine).into_iter();
            names
                .map(|name| (name.clone(), machine.state.accounts[&name].sid.clone()))
                .collect()
        };
        let before = users(&machine);
        assert_eq!(before.len(), 8);
        for run in 0..2 {
            let from = machine.calls.len();
            let report = set_up_fresh(&mut machine, &mut acl);
            assert_eq!(report.made, [], "older {older}, run {run}");
            let calls = machine.calls_since(from);
            let changed = |c: &&str| c.starts_with("create_user") || c.starts_with("delete_user");
            assert!(
                !calls.iter().any(changed),
                "older {older}, run {run}: {calls:?}"
            );
            assert_eq!(machine.state.record.clone().unwrap().pool, eight());
            assert_eq!(users(&machine), before, "older {older}, run {run}");
        }
        for (name, _) in &before {
            assert!(machine.stranded.commented.contains(name), "{name}");
        }
        assert_eq!(machine.stranded.marker_version, Some(MARKER_VERSION));
    }
}

#[test]
fn a_corrupt_record_rebuilt_while_waiting_for_the_lock_is_read_again_and_not_swept() {
    let (mut machine, mut acl) = corrupt("stranded-corrupt-rebuilt", PartialRecord::default());
    // Another Windows user's set-up rebuilds the record while this removal waits.
    let mut rebuilt = machine.m.state.record.clone().unwrap();
    rebuilt.owner = Sid::parse(super::fake::OTHER_OWNER).unwrap();
    machine.after_lock = Some(super::remove_fake::Change::Write(rebuilt));
    let state_accounts = machine.m.state.accounts.clone();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(removed, Removed::NotOwner);
    assert_eq!(machine.m.state.accounts, state_accounts);
    assert_eq!(
        machine.lines(),
        ["read_record", "lock 60000", "read_record", "unlock"]
    );
}

#[test]
fn a_record_sid_outside_this_machines_domain_never_loses_its_rights() {
    let (mut machine, mut acl) = pool("stranded-rights-domain", Some(owner()));
    let users = Sid::parse("S-1-5-32-545").unwrap();
    machine
        .m
        .state
        .record
        .as_mut()
        .unwrap()
        .pool
        .push(users.clone());
    machine.m.stranded.rights.insert(users.clone());
    assert!(matches!(
        remove(&mut machine, &mut acl, false),
        Ok(Removed::All(_))
    ));
    assert!(machine.m.stranded.rights.contains(&users));
    assert!(
        !machine
            .lines()
            .iter()
            .any(|l| l.starts_with("revoke_rights 545"))
    );
    assert!(machine.lines().contains(&"revoke_rights 1001".to_owned()));
}

#[test]
fn a_failed_delete_leaves_an_older_marked_user_provable_for_the_next_run() {
    let (mut machine, mut acl) = no_record("stranded-delete-older");
    machine.m.stranded.commented.clear();
    machine.m.stranded.marker_version = None;
    machine.m.fail("delete_user ClauseoSb03");
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(
        machine.m.state.accounts["ClauseoSb03"].hidden,
        "the sign-in value stays"
    );
    let Removed::All(report) = remove(&mut machine, &mut acl, false).unwrap() else {
        panic!("not removed");
    };
    assert_eq!(report.accounts, 1);
}

#[test]
fn a_user_that_wont_disable_is_named_so_in_the_exit_4_message() {
    let (mut machine, mut acl) = no_record("stranded-disable-fails");
    machine.m.fail("disable ClauseoSb04");
    machine.m.fail("delete_user ClauseoSb04");
    machine.m.fail("delete_user ClauseoSb05");
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(
        failure.cause.contains("ClauseoSb04 (not disabled: "),
        "{}",
        failure.cause
    );
    assert!(
        failure.cause.contains("ClauseoSb05 (disabled)"),
        "{}",
        failure.cause
    );
    assert!(!machine.m.stranded.disabled.contains("ClauseoSb04"));
}
