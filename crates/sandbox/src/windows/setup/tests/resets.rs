//! The profile resets a set-up owes (`resets.rs`): each recorded user released from a matter has
//! its profile deleted, then a logon makes it again, then its `LocalLow` is closed again; only
//! then is its mark cleared, and a reset that fails keeps the mark.

use super::fake::rid;
use super::*;
use crate::windows::pool::PoolUser;

/// The index of the first call `line` in `machine`'s calls since `from`.
fn at(machine: &FakeMachine, from: usize, line: &str) -> usize {
    let calls = machine.calls_since(from);
    calls
        .iter()
        .position(|c| *c == line)
        .unwrap_or_else(|| panic!("no {line} in {calls:?}"))
}

/// A pool of four, set up, with `marked` owed a profile reset and the entries on its `LocalLow`
/// gone, as deleting its profile takes them.
fn pool_with_a_reset_owed(test: &str, marked: u32) -> (FakeMachine, FakeAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.needs_reset.insert(sid(marked));
    (machine, acl)
}

/// The user `ClauseoSb0<n>` with SID `100<n>`.
fn pool_user(n: u32) -> PoolUser {
    PoolUser {
        name: format!("ClauseoSb0{n}"),
        sid: sid(1000 + n),
    }
}

#[test]
fn an_extend_resets_a_marked_user_before_its_logon_and_closes_its_local_low_again() {
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-extend", 1001);
    let old = machine.state.accounts["ClauseoSb01"].password.clone();
    let closed = local_low(&machine, "ClauseoSb01");
    acl.aces.retain(|key| key.0 != closed);
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();

    // Read after the registered matters, as the owner; nothing touched before.
    assert_eq!(
        at(&machine, from, "registered_matters") + 1,
        at(&machine, from, "resets_due")
    );
    // The profile goes, then a new password and a logon make it again; then its LocalLow's
    // owner is checked and the denies written; the mark is cleared once secrets.bin holds the
    // new password, before setup.json.
    let order = [
        "delete_profile 1001",
        "set_password ClauseoSb01",
        "log_on_once ClauseoSb01",
        "owner_of ClauseoSb01",
        "program_data",
        "protect_for_owner 1001,1005,1006,1007,1008",
        "reset_done 1001",
        "write_marker",
    ];
    let indices: Vec<usize> = order.iter().map(|line| at(&machine, from, line)).collect();
    assert!(indices.is_sorted(), "{:?}", machine.calls_since(from));
    assert!(
        acl.has(&closed, &sid(1000), Rights::DenyWrite),
        "its LocalLow is closed again"
    );
    assert!(report.shared.added.iter().any(|grant| grant.path == closed));
    // The others are left as an extend leaves them.
    for n in 2..=4 {
        assert!(
            !machine
                .calls_since(from)
                .contains(&format!("delete_profile 100{n}").as_str())
        );
        assert!(
            !machine
                .calls_since(from)
                .contains(&format!("set_password ClauseoSb0{n}").as_str())
        );
    }
    assert_eq!(report.profiles_reset, Ok(vec![pool_user(1)]));
    assert_eq!(report.reset, [pool_user(1)]);
    assert!(machine.needs_reset.is_empty(), "the mark is cleared");
    let now = &machine.state.accounts["ClauseoSb01"].password;
    assert_ne!(now, &old);
    assert_eq!(
        &machine.state.secrets[&sid(1001)],
        now,
        "secrets.bin has the new one"
    );
    assert_eq!(
        machine.state.profiles[&sid(1001)],
        machine.profile_path("ClauseoSb01")
    );
}

#[test]
fn a_re_run_resets_a_marked_user_too_and_no_other() {
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-rerun", 1003);
    let from = machine.calls.len();
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    let deletes: Vec<&str> = machine
        .calls_since(from)
        .into_iter()
        .filter(|c| c.starts_with("delete_profile"))
        .collect();
    assert_eq!(deletes, ["delete_profile 1003"]);
    assert!(
        at(&machine, from, "delete_profile 1003") < at(&machine, from, "log_on_once ClauseoSb03")
    );
    assert_eq!(report.profiles_reset, Ok(vec![pool_user(3)]));
    assert_eq!(report.reset.len(), 4, "a re-run resets every password");
    assert!(machine.needs_reset.is_empty());
}

#[test]
fn a_fresh_set_up_and_an_extend_with_no_mark_reset_nothing() {
    let mut machine = FakeMachine::new("setup-reset-none");
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);
    assert_eq!(report.profiles_reset, Ok(vec![]));
    assert!(
        !machine.calls.iter().any(|c| c == "resets_due"),
        "no record, no users to reset"
    );
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.profiles_reset, Ok(vec![]));
    assert_eq!(report.reset, []);
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c.starts_with("delete_profile") || c.starts_with("reset_done"))
    );
}

#[test]
fn a_failed_reset_keeps_the_mark() {
    let marked = || std::collections::BTreeSet::from([sid(1002)]);
    // The profile delete fails: the set-up fails there, rolled back, and nothing is cleared.
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-delete", 1002);
    machine.fail("delete_profile 1002");
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.rolled_back),
        (Step::ResetProfile, true)
    );
    assert_eq!(machine.needs_reset, marked());
    assert!(!machine.calls.iter().any(|c| c.starts_with("reset_done")));
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c == "set_password ClauseoSb02")
    );

    // The logon after the delete fails: the mark stays, and the rollback saves the new
    // password, so the pool still works.
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-logon", 1002);
    machine.fail("log_on_once ClauseoSb02");
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::LogOn);
    assert_eq!(machine.needs_reset, marked());
    let now = &machine.state.accounts["ClauseoSb02"].password;
    assert_eq!(&machine.state.secrets[&sid(1002)], now);

    // Its LocalLow can't be closed again: the mark stays.
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-deny", 1002);
    acl.fail_add = Some(local_low(&machine, "ClauseoSb02"));
    acl.aces
        .retain(|key| key.0 != local_low(&machine, "ClauseoSb02"));
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::SharedWrites);
    assert_eq!(machine.needs_reset, marked());

    // The marks can't be cleared after a reset that worked: the set-up stands, says so, and
    // the user stays held back until the next one.
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-clear", 1002);
    machine.fail("reset_done 1002");
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    let Err(why) = &report.profiles_reset else {
        panic!("{:?}", report.profiles_reset);
    };
    assert!(why.contains("injected: reset_done"), "{why}");
    assert_eq!(machine.needs_reset, marked());
    assert!(machine.state.marker.is_some());
    assert_eq!(rid(&report.made[0].sid), "1005");
}

#[test]
fn unreadable_marks_fail_the_set_up_before_any_change() {
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-unreadable", 1001);
    machine.fail("resets_due");
    let before = machine.state.clone();
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::Matters, true));
    assert_eq!(machine.state, before);
}
