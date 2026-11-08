//! A killed run's orphan accounts (`orphans.rs`): a pool-named account in the pool's group
//! that no record names is deleted and its name made again; any other account under a pool
//! name stays, skipped (D-B5). A first set-up killed before its record left a group too: one
//! with our comment and only such accounts is reused; an administrator's is refused.

use std::collections::BTreeSet;

use super::fake::Account;
use super::*;
use crate::windows::setup::orphans::{Found, is_orphan};

/// `machine` set up (`ClauseoSb01` to `08`, the group `sid(1000)`), then as a killed extend
/// left it: the account `ClauseoSb09` with `account`, its SID, in the group when `in_group`.
fn left(machine: &mut FakeMachine, account: &Sid, in_group: bool) {
    set_up_fresh(machine, &mut FakeAcl::default());
    let groups = if in_group {
        BTreeSet::from([sid(1000)])
    } else {
        BTreeSet::new()
    };
    machine.state.accounts.insert(
        "ClauseoSb09".to_owned(),
        Account {
            sid: account.clone(),
            password: "lost".to_owned(),
            groups,
            hidden: true,
        },
    );
}

/// An extend by two on `machine`: the new users' names, and the names skipped.
fn extend(machine: &mut FakeMachine) -> (Vec<String>, Vec<(String, Sid)>) {
    let req = request(machine, 2, Vec::new());
    let report = set_up(machine, &mut FakeAcl::default(), &req).unwrap();
    let made = report.made.iter().map(|user| user.name.clone()).collect();
    (made, report.skipped_names)
}

#[test]
fn a_killed_runs_account_in_the_pools_group_is_deleted_and_its_name_made_again() {
    let mut machine = FakeMachine::new("setup-orphan");
    let orphan = sid(1900);
    left(&mut machine, &orphan, true);
    let from = machine.calls.len();
    let (made, skipped) = extend(&mut machine);
    assert_eq!(
        made,
        ["ClauseoSb09", "ClauseoSb10"],
        "ClauseoSb09 is used again"
    );
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_ne!(
        machine.state.accounts["ClauseoSb09"].sid, orphan,
        "a new account"
    );
    assert!(!machine.state.record.clone().unwrap().pool.contains(&orphan));
    let calls = machine.calls_since(from);
    let at = |call: &str| calls.iter().position(|made| *made == call);
    let deleted = at("delete_user ClauseoSb09").expect("the orphan deleted");
    assert!(at("in_group 1900 1000") < Some(deleted), "{calls:?}");
    assert!(Some(deleted) < at("create_user ClauseoSb09"), "{calls:?}");
}

#[test]
fn an_account_outside_the_group_or_not_this_machines_is_skipped_never_deleted() {
    for (account, in_group) in [
        // Killed before the group was set: not plainly set-up's, so it stays.
        (sid(1900), false),
        // Another domain's account, whatever its groups.
        (Sid::parse("S-1-5-21-7-7-7-1900").unwrap(), true),
    ] {
        let mut machine = FakeMachine::new("setup-orphan-kept");
        left(&mut machine, &account, in_group);
        let (made, skipped) = extend(&mut machine);
        assert_eq!(made, ["ClauseoSb10", "ClauseoSb11"]);
        assert_eq!(skipped, [("ClauseoSb09".to_owned(), account.clone())]);
        assert_eq!(machine.state.accounts["ClauseoSb09"].sid, account);
        assert!(
            !machine
                .calls
                .iter()
                .any(|call| call == "delete_user ClauseoSb09")
        );
    }
}

#[test]
fn only_a_pool_named_local_account_in_the_group_that_no_record_names_is_an_orphan() {
    let (group, owner, own) = (sid(1000), sid(1001), sid(1002));
    let check = |name: &str, account: &Sid, in_group: bool, recorded: &[Sid]| {
        let found = Found {
            name,
            sid: account,
            in_group,
        };
        is_orphan(found, &group, recorded, &own, &owner)
    };
    let orphan = sid(1900);
    assert!(check("ClauseoSb07", &orphan, true, &[]));
    assert!(
        !check("ClauseoSb07", &orphan, false, &[]),
        "not in the group"
    );
    assert!(
        !check("ClauseoSb07", &orphan, true, std::slice::from_ref(&orphan)),
        "recorded"
    );
    for name in [
        "Rohan",
        "ClauseoSb00",
        "ClauseoSb33",
        "ClauseoSb7",
        "ClauseoSandboxUsers",
    ] {
        assert!(!check(name, &orphan, true, &[]), "{name}");
    }
    for account in [
        owner.clone(),
        own.clone(),
        sid(500),
        Sid::parse("S-1-5-21-7-7-7-1900").unwrap(),
        Sid::parse("S-1-12-1-1-2-3-4").unwrap(),
        Sid::parse("S-1-5-18").unwrap(),
    ] {
        assert!(!check("ClauseoSb07", &account, true, &[]), "{account}");
    }
}

/// `machine` as a first set-up killed before its record left it: the group `sid(1000)`, with
/// the accounts `members` (name, SID) in it, and no record.
fn first_run_left(machine: &mut FakeMachine, members: &[(&str, Sid)]) {
    machine.state.group = Some(sid(1000));
    machine.next_rid = 1100;
    for (name, account) in members {
        let in_group = BTreeSet::from([sid(1000)]);
        let entry = machine.state.accounts.entry((*name).to_owned());
        let held = entry.or_insert_with(|| Account {
            sid: account.clone(),
            password: "lost".to_owned(),
            groups: BTreeSet::new(),
            hidden: true,
        });
        held.groups.extend(in_group);
    }
}

#[test]
fn a_first_run_killed_before_its_record_leaves_a_group_the_next_run_reuses() {
    // Its own users in it, or none (killed before the first was made).
    for members in [
        vec![("ClauseoSb01", sid(1001)), ("ClauseoSb02", sid(1002))],
        Vec::new(),
    ] {
        let mut machine = FakeMachine::new("setup-first-run-group");
        first_run_left(&mut machine, &members);
        let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
        assert!(!report.group_made, "reused, not made");
        let record = machine.state.record.clone().unwrap();
        assert_eq!(record.group, sid(1000));
        let calls = &machine.calls;
        assert!(calls.iter().all(|call| call != "create_group"), "{calls:?}");
        assert!(calls.contains(&"group_facts 1000".to_owned()), "{calls:?}");
        for (name, old) in &members {
            assert!(
                !record.pool.contains(old),
                "{name}'s old account isn't kept"
            );
            assert_ne!(&machine.state.accounts[*name].sid, old, "{name} made again");
            let at = |call: String| calls.iter().position(|made| *made == call);
            let deleted = at(format!("delete_user {name}")).expect("deleted");
            assert!(
                Some(deleted) < at(format!("create_user {name}")),
                "{calls:?}"
            );
        }
        let names: Vec<&str> = report.pool.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names.first(), Some(&"ClauseoSb01"));
    }
}

#[test]
fn a_group_with_another_member_or_comment_is_an_admins_and_still_refused() {
    let refused = |machine: &mut FakeMachine| {
        let before = machine.state.clone();
        let req = request(machine, 0, Vec::new());
        let failure = set_up(machine, &mut FakeAcl::default(), &req).unwrap_err();
        assert_eq!(failure.step, Step::Group);
        assert!(
            failure.cause.contains("that set-up didn't record"),
            "{}",
            failure.cause
        );
        assert_eq!(machine.state, before, "nothing deleted or made");
        assert!(
            !machine
                .calls
                .iter()
                .any(|call| call.starts_with("delete_user"))
        );
    };
    // The owner in it, beside a pool-shaped account: an admin's group.
    let mut machine = FakeMachine::new("setup-admins-group");
    let owner = Sid::parse(OWNER).unwrap();
    first_run_left(
        &mut machine,
        &[("ClauseoSb01", sid(1001)), (OWNER_NAME, owner)],
    );
    refused(&mut machine);
    // A pool name on another machine's or a built-in account.
    for stranger in [Sid::parse("S-1-5-21-7-7-7-1001").unwrap(), sid(500)] {
        let mut machine = FakeMachine::new("setup-admins-group-stranger");
        first_run_left(&mut machine, &[("ClauseoSb01", stranger)]);
        refused(&mut machine);
    }
    // Empty, but with a comment set-up never writes.
    let mut machine = FakeMachine::new("setup-admins-group-comment");
    first_run_left(&mut machine, &[]);
    machine.state.group_comment = Some("Lab machines".to_owned());
    refused(&mut machine);
}
