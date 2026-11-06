//! What a rollback leaves, and that the record names all of it: a failed fence, made users the
//! ledger names, a group or user that won't go, a group remade by a re-run or an extend; the
//! re-run's passwords; the cap with dead SIDs kept; the lock given back on a crash.

use super::fake::State;
use super::*;
use crate::windows::pool::MachineRecord;

/// A machine with a fresh pool of four and two registered matters.
fn pool_with_matters(test: &str) -> (FakeMachine, FakeAcl, Vec<RegisteredMatter>) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let matters = two_matters(&machine);
    (machine, acl, matters)
}

/// The record a fresh run that kept only `pool` and the group 1000 leaves.
fn recorded(machine: &FakeMachine, pool: Vec<Sid>) -> MachineRecord {
    MachineRecord {
        owner: owner(),
        group: sid(1000),
        pool,
        data_dir: machine.data_dir(),
        removing: false,
    }
}

/// Every account's blob in `secrets.bin` is its password now.
fn secrets_match(state: &State) {
    for (name, account) in &state.accounts {
        if name.starts_with("ClauseoSb") {
            assert_eq!(
                state.secrets.get(&account.sid),
                Some(&account.password),
                "{name}"
            );
        }
    }
}

#[test]
fn a_failed_fence_rolls_back_the_new_users_with_matters_registered() {
    let (mut machine, mut acl, matters) = pool_with_matters("setup-fence-fails");
    acl.fail_add = Some(dunce::canonicalize(&matters[1].folders.root).unwrap());
    let before = machine.state.clone();
    let req = request(&mut machine, 4, matters);
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Fence);
    assert!(failure.rolled_back, "{}", failure.cause);
    // The fence's own rollback left no ledger entry naming the new users, so the record is as
    // it was, and so is the machine.
    assert_eq!(machine.state, before);
}

#[test]
fn made_users_the_ledger_names_stay_in_the_record_after_a_rollback() {
    let (mut machine, mut acl, matters) = pool_with_matters("setup-ledger-keeps");
    machine.fail("write_marker");
    let req = request(&mut machine, 4, matters);
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Marker);
    assert!(failure.rolled_back, "{}", failure.cause);
    for n in 5..=8 {
        assert!(
            !machine
                .state
                .accounts
                .contains_key(&format!("ClauseoSb0{n}"))
        );
    }
    // The fence's entries name the new SIDs, so the record keeps them for removal.
    let pool = machine.state.record.clone().unwrap().pool;
    assert_eq!(pool, (1001..=1008).map(sid).collect::<Vec<_>>());
}

#[test]
fn a_group_that_wont_go_stays_recorded() {
    for call in ["group_members 1000", "delete_group 1000"] {
        let mut machine = FakeMachine::new("setup-group-stays");
        machine.fail("write_marker");
        machine.fail(call);
        let req = request(&mut machine, 0, Vec::new());
        let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
        assert!(!failure.rolled_back, "{call}");
        assert!(
            failure.cause.contains("ClauseoSandboxUsers"),
            "{call}: {}",
            failure.cause
        );
        assert_eq!(machine.state.group, Some(sid(1000)), "{call}");
        assert_eq!(
            machine.state.record,
            Some(recorded(&machine, vec![])),
            "{call}"
        );
        let rest = machine.calls_since(machine.calls.iter().position(|c| c == call).unwrap());
        assert!(!rest.contains(&"delete_record"), "{call}: {rest:?}");
    }
}

#[test]
fn a_user_that_wont_go_before_any_record_write_is_recorded() {
    let mut machine = FakeMachine::new("setup-unrecorded-user");
    machine.fail("hide ClauseoSb01");
    machine.fail("delete_user ClauseoSb01");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!((failure.step, failure.rolled_back), (Step::Hide, false));
    assert!(machine.state.accounts.contains_key("ClauseoSb01"));
    assert_eq!(
        machine.state.record,
        Some(recorded(&machine, vec![sid(1001)]))
    );
    assert_eq!(machine.state.group, Some(sid(1000)), "it has a member");
}

/// A pool whose group was deleted by hand.
fn without_group(test: &str) -> FakeMachine {
    let mut machine = FakeMachine::new(test);
    set_up_fresh(&mut machine, &mut FakeAcl::default());
    machine.state.group = None;
    for account in machine.state.accounts.values_mut() {
        account.groups.clear();
    }
    machine
}

#[test]
fn a_group_an_extend_remade_stays_recorded_after_a_failure() {
    let mut machine = without_group("setup-extend-regroup");
    machine.fail("create_user ClauseoSb05");
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::CreateUser);
    assert!(failure.rolled_back, "{}", failure.cause);
    let group = machine.state.group.clone().unwrap();
    assert_eq!(group, sid(1005), "the kept users are in it, so it stays");
    let record = machine.state.record.clone().unwrap();
    assert_eq!(record.group, group);
    assert_eq!(record.pool, (1001..=1004).map(sid).collect::<Vec<_>>());
}

#[test]
fn a_group_a_re_run_remade_stays_recorded_and_the_new_passwords_are_saved() {
    let mut machine = without_group("setup-rerun-regroup");
    machine.fail("log_on_once ClauseoSb02");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::LogOn);
    assert!(failure.rolled_back, "{}", failure.cause);
    let group = machine.state.group.clone().unwrap();
    assert_eq!(machine.state.record.clone().unwrap().group, group);
    // Two passwords were reset before the failure; `secrets.bin` holds them, and the old blobs
    // of the two not reset.
    let last = machine
        .calls
        .iter()
        .rev()
        .find(|c| c.starts_with("protect_for_owner"));
    assert_eq!(
        last.map(String::as_str),
        Some("protect_for_owner 1001,1002")
    );
    secrets_match(&machine.state);
}

#[test]
fn the_run_after_a_failed_re_run_saves_every_users_password() {
    let mut machine = FakeMachine::new("setup-rerun-then-ok");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    machine.fail("log_on_once ClauseoSb03");
    let req = request(&mut machine, 0, Vec::new());
    set_up(&mut machine, &mut acl, &req).unwrap_err();
    secrets_match(&machine.state);
    let from = machine.calls.len();
    set_up(&mut machine, &mut acl, &req).unwrap();
    assert!(
        machine
            .calls_since(from)
            .contains(&"protect_for_owner 1001,1002,1003,1004")
    );
    secrets_match(&machine.state);
    assert_eq!(machine.state.secrets.len(), 4);
}

#[test]
fn dead_sids_the_record_keeps_count_toward_the_cap() {
    let (mut machine, mut acl, matters) = pool_with_matters("setup-cap-dead");
    let req = request(&mut machine, 28, matters.clone());
    set_up(&mut machine, &mut acl, &req).unwrap();
    machine.state.accounts.remove("ClauseoSb10");
    let req = request(&mut machine, 0, matters.clone());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.kept_dead, [sid(1010)]);
    assert_eq!(report.made, [], "the record has no room for one more");
    assert_eq!(machine.state.record.clone().unwrap().pool.len(), 32);
    let req = request(&mut machine, 4, matters);
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Pool);
    assert_eq!(machine.state.record.clone().unwrap().pool.len(), 32);
}

#[test]
fn a_crash_inside_the_run_still_gives_the_lock_back() {
    let mut machine = FakeMachine::new("setup-crash-lock");
    machine.panic_on = Some("create_user ClauseoSb02".to_owned());
    let req = request(&mut machine, 0, Vec::new());
    let crashed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        set_up(&mut machine, &mut FakeAcl::default(), &req)
    }));
    assert!(crashed.is_err());
    assert_eq!(machine.calls.last().map(String::as_str), Some("unlock"));
}

#[test]
fn secrets_bin_that_cant_be_written_in_a_rollback_stays_as_it_was() {
    let mut machine = FakeMachine::new("setup-rerun-secrets-fail");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let old = machine.state.secrets.clone();
    // Step 10 fails, then the rollback's own write of the new passwords fails too.
    machine.fail("protect_for_owner 1001,1002,1003,1004");
    machine.fail("protect_for_owner 1001,1002,1003,1004");
    let req = request(&mut machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Secrets);
    assert!(!failure.rolled_back);
    assert!(
        failure.cause.contains("secrets.bin") && failure.cause.contains("run set-up again"),
        "{}",
        failure.cause
    );
    let tries = machine
        .calls
        .iter()
        .filter(|c| c.starts_with("protect_for_owner"))
        .count();
    assert_eq!(tries, 3, "the fresh run's, step 10's and the rollback's");
    assert_eq!(machine.state.secrets, old, "the old secrets.bin stays");
}
