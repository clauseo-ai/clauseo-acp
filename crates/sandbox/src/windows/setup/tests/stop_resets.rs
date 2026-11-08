//! The stop task's marks (`ResetSids`, `resets.rs`): a set-up resets each listed pool user, even
//! one a matter holds, once no session is open, and takes it off the list; read and cleared as
//! admin, never as the owner.

use std::collections::BTreeSet;

use super::fake::rid;
use super::*;

/// A pool of eight, set up, whose record's `ResetSids` lists `listed`, with 1002 held by a
/// matter and not marked in `matters.json`.
fn pool_with_stop_marks(test: &str, listed: &[u32]) -> (FakeMachine, FakeAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.held.insert(sid(1002));
    machine.stop_resets = listed.iter().map(|n| sid(*n)).collect();
    (machine, acl)
}

/// The SIDs (as relative ids) whose profiles `machine` deleted since `from`.
fn deleted(machine: &FakeMachine, from: usize) -> Vec<String> {
    let calls = machine.calls_since(from);
    let deletes = calls
        .iter()
        .filter_map(|c| c.strip_prefix("delete_profile "));
    deletes.map(str::to_owned).collect()
}

#[test]
fn an_extend_resets_a_held_user_the_stop_task_listed_and_takes_it_off() {
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-held", &[1002]);
    let old = machine.state.accounts["ClauseoSb02"].password.clone();
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    // Read as admin after matters.json's marks; then the open sessions, as the owner.
    let calls = machine.calls_since(from);
    let read = calls.iter().position(|c| *c == "stop_resets").unwrap();
    assert_eq!(calls[read - 1], "resets_due");
    assert_eq!(calls[read + 1], "sessions_open");
    assert_eq!(deleted(&machine, from), ["1002"]);
    assert_eq!(report.profiles_reset.as_ref().map(Vec::len), Ok(1));
    let now = &machine.state.accounts["ClauseoSb02"].password;
    assert_ne!(now, &old, "a new password");
    assert_eq!(&machine.state.secrets[&sid(1002)], now);
    assert!(machine.stop_resets.is_empty(), "taken off ResetSids");
    assert!(calls.contains(&"stop_resets_done 1002"), "{calls:?}");
    assert!(
        !machine
            .outside_owner
            .iter()
            .any(|c| c.contains("stop_resets")),
        "never through the owner"
    );
}

#[test]
fn while_a_session_is_open_the_listed_users_wait_and_stay_listed() {
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-open", &[1002, 1003]);
    machine.sessions_open = true;
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(deleted(&machine, from), Vec::<String>::new());
    assert_eq!(report.profiles_reset, Ok(vec![]));
    assert_eq!(machine.stop_resets, BTreeSet::from([sid(1002), sid(1003)]));
    // An unreadable open-session record waits too, rather than failing the set-up.
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-unread", &[1003]);
    machine.fail("sessions_open");
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(deleted(&machine, from), Vec::<String>::new());
    assert_eq!(machine.stop_resets, BTreeSet::from([sid(1003)]));
}

#[test]
fn only_the_records_pool_users_are_taken_from_the_list_and_with_matters_marks_once() {
    // 1999 isn't in the record's pool: never reset, left listed. 1003 is marked both ways.
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-pool", &[1003, 1999]);
    machine.needs_reset.insert(sid(1003));
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(deleted(&machine, from), ["1003"]);
    let reset: Vec<String> = report
        .profiles_reset
        .unwrap()
        .iter()
        .map(|user| rid(&user.sid))
        .collect();
    assert_eq!(reset, ["1003"]);
    assert!(machine.needs_reset.is_empty());
    assert_eq!(machine.stop_resets, BTreeSet::from([sid(1999)]));
}

#[test]
fn a_list_that_cant_be_read_or_cleared_keeps_the_marks() {
    // Unreadable: the set-up stops before anything changes.
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-read", &[1003]);
    machine.fail("stop_resets");
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Matters);
    assert_eq!(deleted(&machine, from), Vec::<String>::new());
    // Reset, but not taken off: the set-up stands and says so; the user stays listed.
    let (mut machine, mut acl) = pool_with_stop_marks("setup-stop-reset-clear", &[1003]);
    machine.fail("stop_resets_done 1003");
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    let Err(why) = &report.profiles_reset else {
        panic!("{:?}", report.profiles_reset);
    };
    assert!(why.contains("injected: stop_resets_done"), "{why}");
    assert_eq!(machine.stop_resets, BTreeSet::from([sid(1003)]));
}
