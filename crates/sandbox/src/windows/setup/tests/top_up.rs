//! The top-up (`top_up.rs`): every run leaves at least [`MIN_FREE_AFTER_RUN`] free pool users,
//! cap allowing, on top of what the run does anyway: a re-run stays a re-run.

use super::*;

/// A fresh pool of eight, with the users `held` (relative ids) in use by a matter or a grant.
fn pool_holding(test: &str, held: impl IntoIterator<Item = u32>) -> (FakeMachine, FakeAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.held.extend(held.into_iter().map(sid));
    (machine, acl)
}

/// The top-up a report gives.
fn top_up(free_before: usize, added: usize, short: usize) -> TopUp {
    TopUp {
        free_before,
        added,
        short,
    }
}

/// Every pool user's password now, by name.
fn passwords(machine: &FakeMachine) -> Vec<(String, String)> {
    let accounts = machine.state.accounts.iter();
    let pool = accounts.filter(|(name, _)| name.starts_with("ClauseoSb"));
    pool.map(|(name, a)| (name.clone(), a.password.clone()))
        .collect()
}

#[test]
fn a_fresh_set_up_makes_eight() {
    let mut machine = FakeMachine::new("topup-fresh");
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(report.made.len(), MIN_FREE_AFTER_RUN);
    assert_eq!(report.top_up, top_up(0, MIN_FREE_AFTER_RUN - POOL_START, 0));
    assert!(
        !machine.calls.iter().any(|c| c == "held_users"),
        "no record, no users in use"
    );
}

#[test]
fn three_marked_and_two_free_make_five_free_so_it_adds_three() {
    // 1001 to 1003 in use, 1004 to 1006 marked, 1007 and 1008 free.
    let (mut machine, mut acl) = pool_holding("topup-marked", 1001..=1003);
    machine.needs_reset.extend((1004..=1006).map(sid));
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(5, 3, 0));
    let made: Vec<Sid> = report.made.iter().map(|u| u.sid.clone()).collect();
    assert_eq!(made, (1009..=1011).map(sid).collect::<Vec<_>>());
    assert_eq!(report.reset.len(), 8, "a re-run resets every kept password");
    let profiles = report.profiles_reset.unwrap().into_iter();
    let profiles: Vec<Sid> = profiles.map(|u| u.sid).collect();
    assert_eq!(
        profiles,
        (1004..=1006).map(sid).collect::<Vec<_>>(),
        "the marked alone"
    );
    assert!(machine.needs_reset.is_empty());
    assert_eq!(machine.state.record.clone().unwrap().pool.len(), 11);
    let resets: Vec<Sid> = (1004..=1006).map(sid).collect();
    assert_eq!(
        machine.held_for,
        [resets],
        "counted against the resets it acts on"
    );
}

#[test]
fn every_user_held_adds_the_full_shortfall() {
    // As an unreadable ledger, session lock or set-aside manifest leaves it.
    let (mut machine, mut acl) = pool_holding("topup-all-held", 1001..=1008);
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(0, 8, 0));
    assert_eq!(report.made.len(), 8);
}

#[test]
fn eight_or_more_free_add_none() {
    let (mut machine, mut acl) = pool_holding("topup-enough", []);
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(8, 0, 0));
    assert_eq!(report.made, []);
    assert_eq!(report.reset.len(), 8, "a plain re-run, as before");

    // An extend of 4 over 8 free adds its 4 and no more.
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(8, 0, 0));
    assert_eq!(report.made.len(), 4);
}

#[test]
fn a_requested_add_is_a_minimum() {
    // Six of eight in use: two free, so six short; an extend of 4 adds 6.
    let (mut machine, mut acl) = pool_holding("topup-add-min", 1001..=1006);
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.made.len(), 6);
    assert_eq!(report.top_up, top_up(2, 2, 0));
    assert_eq!(report.reset, [], "an extend resets no password");
}

#[test]
fn near_the_cap_it_adds_what_fits_and_reports_the_rest() {
    let (mut machine, mut acl) = pool_holding("topup-cap", []);
    let req = request(&mut machine, 20, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap();
    // 28 users, 26 in use: six short, room for four.
    machine.held.extend((1001..=1026).map(sid));
    let req = request(&mut machine, 0, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(2, 4, 2));
    assert_eq!(report.made.len(), 4);
    assert_eq!(machine.state.record.clone().unwrap().pool.len(), 32);

    // At the cap: nothing to add, the shortfall reported, no failure.
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(6, 0, 2));
    assert_eq!(report.made, []);
}

#[test]
fn a_re_run_that_tops_up_is_still_a_re_run() {
    let (mut machine, mut acl) = pool_holding("topup-rerun", 1001..=1004);
    machine.sessions_open = true;
    let before = machine.state.clone();
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::Sessions, true));
    assert_eq!(
        machine.state, before,
        "refused while a session is open, as today"
    );

    machine.sessions_open = false;
    let old = passwords(&machine);
    let from = machine.calls.len();
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.top_up, top_up(4, 4, 0));
    assert_eq!(report.reset.len(), 8, "every kept password, as today");
    assert_eq!(report.made.len(), 4);
    let calls = machine.calls_since(from);
    let all = "protect_for_owner 1001,1002,1003,1004,1005,1006,1007,1008,1009,1010,1011,1012";
    assert!(calls.contains(&all), "{calls:?}");
    let now = passwords(&machine);
    for (name, password) in &old {
        assert!(
            !now.contains(&(name.clone(), password.clone())),
            "{name} reset"
        );
        let sid = &machine.state.accounts[name].sid;
        let now = &machine.state.accounts[name].password;
        assert_eq!(&machine.state.secrets[sid], now, "{name}'s new blob");
    }
    assert_eq!(machine.state.secrets.len(), 12);
}

#[test]
fn a_failure_while_adding_rolls_back_the_new_users_and_keeps_the_marks() {
    let (mut machine, mut acl) = pool_holding("topup-fails", 1001..=1004);
    machine.needs_reset.insert(sid(1005));
    machine.fail("log_on_once ClauseoSb10");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::LogOn, true));
    for name in ["ClauseoSb09", "ClauseoSb10"] {
        assert!(!machine.state.accounts.contains_key(name), "{name}");
    }
    let pool = machine.state.record.clone().unwrap().pool;
    assert_eq!(pool, (1001..=1008).map(sid).collect::<Vec<_>>());
    assert_eq!(machine.needs_reset, [sid(1005)].into(), "the mark stays");
    assert!(!machine.calls.iter().any(|c| c.starts_with("reset_done")));
}
