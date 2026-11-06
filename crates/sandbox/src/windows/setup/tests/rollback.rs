//! The rollback (D-B7): a failure injected at each step of a fresh run, the record write's own
//! rule, the shared writes' failure, and a rollback that can't finish.

use super::fake::State;
use super::*;

/// The step a failed call reports.
fn step_of(call: &str) -> Step {
    match call.split(' ').next().unwrap() {
        "read_record" => Step::ReadRecord,
        "seclogon" => Step::SecondaryLogon,
        "logon_right" => Step::LogonRight,
        "group" | "create_group" => Step::Group,
        "account_sid" | "create_user" => Step::CreateUser,
        "add_to_group" => Step::AddToGroup,
        "hide" => Step::Hide,
        "write_record" => Step::WriteRecord,
        "log_on_once" => Step::LogOn,
        "program_data" => Step::SharedWrites,
        "protect_for_owner" => Step::Secrets,
        "write_marker" => Step::Marker,
        other => panic!("no step for {other}"),
    }
}

/// A fresh run with `call` failing: its failure, the machine and the writer after.
fn failing_at(test: &str, call: &str) -> (SetupFailure, FakeMachine, FakeAcl, State) {
    let mut machine = FakeMachine::new(test);
    machine.fail(call);
    let before = machine.state.clone();
    let mut acl = FakeAcl::default();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    (failure, machine, acl, before)
}

#[test]
fn a_failure_at_each_step_rolls_back_to_the_machine_before() {
    let calls = fresh_calls();
    let first_record = calls
        .iter()
        .position(|c| c.starts_with("write_record"))
        .unwrap();
    for (at, call) in calls.iter().enumerate() {
        // The lock has its own test; the journal never fails the set-up.
        if call == "lock"
            || call == "unlock"
            || call.starts_with("journal")
            || call.starts_with("grow")
        {
            continue;
        }
        let (failure, machine, acl, before) = failing_at("setup-each-step", call);
        assert_eq!(failure.step, step_of(call), "{call}");
        assert_eq!(failure.exit, SetupExit::Failed, "{call}");
        assert!(failure.rolled_back, "{call}: {}", failure.cause);
        assert!(
            failure.cause.contains("injected"),
            "{call}: {}",
            failure.cause
        );
        // `secrets.bin` written before a failed marker keeps blobs for users now gone, which
        // nothing can use; everything else is as it was.
        let mut after = machine.state.clone();
        if call == "write_marker" {
            assert_eq!(after.secrets.len(), 4);
            after.secrets.clear();
        }
        assert_eq!(after, before, "{call}: the machine as it was");
        assert_eq!(
            machine.calls.last().map(String::as_str),
            Some("unlock"),
            "{call}"
        );
        assert_eq!(
            &machine.calls[..=at],
            &calls[..=at],
            "{call}: the same calls up to it"
        );
        let rest = machine.calls_since(at + 1);
        if at < first_record {
            assert!(
                !rest.iter().any(|c| c.contains("record")),
                "{call}: no record written before step 5: {rest:?}"
            );
        }
        // Every made user comes off in the uninstall order: profile, then account.
        let made: Vec<&str> = calls[..at]
            .iter()
            .filter_map(|c| c.strip_prefix("create_user "))
            .collect();
        for name in &made {
            let deleted = rest
                .iter()
                .position(|c| *c == format!("delete_user {name}"));
            assert!(deleted.is_some(), "{call}: {name} deleted: {rest:?}");
        }
        // The group this run made goes; `DeviceSync`'s deny stays once written; no `LocalLow`
        // deny is left.
        if calls[..at].iter().any(|c| c == "create_group") {
            assert!(rest.contains(&"delete_group 1000"), "{call}: {rest:?}");
        }
        assert!(
            acl.aces
                .iter()
                .all(|(path, ..)| *path == machine.device_sync()),
            "{call}: {:?}",
            acl.aces
        );
    }
}

#[test]
fn a_record_write_failure_deletes_exactly_that_user() {
    let (failure, machine, _, before) = failing_at("setup-record-first", "write_record 1001");
    assert_eq!(failure.step, Step::WriteRecord);
    let from = machine
        .calls
        .iter()
        .position(|c| c == "write_record 1001")
        .unwrap();
    assert_eq!(
        machine.calls_since(from + 1),
        [
            "delete_user ClauseoSb01",
            "group_members 1000",
            "delete_group 1000",
            "unlock"
        ]
    );
    assert_eq!(machine.state, before);

    // The second user's write fails: that user goes (it has no profile yet), then the first
    // user as every user this run made, and the record it was in.
    let (_, machine, _, before) = failing_at("setup-record-second", "write_record 1001,1002");
    let from = machine
        .calls
        .iter()
        .position(|c| c == "write_record 1001,1002")
        .unwrap();
    assert_eq!(
        machine.calls_since(from + 1),
        [
            "delete_user ClauseoSb02",
            "delete_profile 1001",
            "delete_user ClauseoSb01",
            "group_members 1000",
            "delete_record",
            "delete_group 1000",
            "unlock",
        ]
    );
    assert_eq!(machine.state, before);
}

#[test]
fn a_failed_logon_takes_off_the_profiles_then_the_accounts() {
    let (failure, machine, _, before) = failing_at("setup-logon-fails", "log_on_once ClauseoSb02");
    assert_eq!(failure.step, Step::LogOn);
    let from = machine
        .calls
        .iter()
        .position(|c| c == "log_on_once ClauseoSb02")
        .unwrap();
    assert_eq!(
        machine.calls_since(from + 1),
        [
            "delete_profile 1002",
            "delete_user ClauseoSb02",
            "delete_profile 1001",
            "delete_user ClauseoSb01",
            "group_members 1000",
            "delete_record",
            "delete_group 1000",
            "unlock",
        ]
    );
    assert_eq!(machine.state, before);
}

#[test]
fn a_failed_shared_write_fails_the_set_up_and_keeps_the_deny() {
    let mut machine = FakeMachine::new("setup-shared-fails");
    let mut acl = FakeAcl {
        fail_walk: Some(machine.device_sync()),
        ..FakeAcl::default()
    };
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::SharedWrites);
    let device_sync = machine.device_sync();
    assert!(
        failure.cause.contains(&device_sync.display().to_string()),
        "{}",
        failure.cause
    );
    assert!(failure.rolled_back, "{}", failure.cause);
    // The deny on the failed target stays (D12); each rolled-back user's `LocalLow` deny came
    // off before its profile went.
    assert!(acl.has(&device_sync, &sid(1000), Rights::DenyWrite));
    for name in ["ClauseoSb01", "ClauseoSb02", "ClauseoSb03", "ClauseoSb04"] {
        let local_low = local_low(&machine, name);
        assert!(
            acl.aces.iter().all(|(path, ..)| *path != local_low),
            "{name}"
        );
        assert!(
            acl.removed.iter().any(|(path, ..)| *path == local_low),
            "{name}"
        );
    }
    assert!(
        !machine
            .calls
            .iter()
            .any(|c| c == "protect_for_owner" || c == "write_marker")
    );
    assert_eq!(machine.state, before);
}

#[test]
fn a_user_the_rollback_cannot_delete_stays_recorded() {
    let mut machine = FakeMachine::new("setup-rollback-partial");
    machine.fail("write_marker");
    machine.fail("delete_user ClauseoSb03");
    let mut acl = FakeAcl::default();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Marker);
    assert!(!failure.rolled_back);
    assert!(failure.cause.contains("ClauseoSb03"), "{}", failure.cause);
    // Only the survivor is recorded, with the group, which isn't empty and so stays.
    let record = machine.state.record.clone().unwrap();
    assert_eq!(record.pool, [sid(1003)]);
    assert_eq!(record.group, sid(1000));
    assert_eq!(machine.state.group, Some(sid(1000)));
    let names: Vec<&String> = machine.state.accounts.keys().collect();
    assert_eq!(names, ["ClauseoSb03", OWNER_NAME]);
    assert!(!machine.calls.iter().any(|c| c.starts_with("delete_group")));
}

#[test]
fn a_failed_re_run_keeps_every_account_that_was_there() {
    let mut machine = FakeMachine::new("setup-rerun-fails");
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let record = machine.state.record.clone();
    machine.fail("write_marker");
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    assert_eq!(failure.step, Step::Marker);
    assert!(failure.rolled_back);
    assert_eq!(
        machine.state.record, record,
        "nothing recorded before is dropped"
    );
    for n in 1..=4 {
        assert!(
            machine
                .state
                .accounts
                .contains_key(&format!("ClauseoSb0{n}"))
        );
    }
    assert!(!machine.calls.iter().any(|c| c.starts_with("delete_")));
    assert!(acl.has(
        &local_low(&machine, "ClauseoSb01"),
        &sid(1000),
        Rights::DenyWrite
    ));
}
