//! A fresh set-up: its call order, the preflight's refusals, the owner checks, the name scheme
//! and the exit codes.

use super::fake::{OTHER_OWNER, VOLUME};
use super::*;
use crate::SetupError;
use crate::windows::pool::MachineRecord;

#[test]
fn a_fresh_run_calls_every_step_in_order() {
    let mut machine = FakeMachine::new("setup-fresh");
    let mut acl = FakeAcl::default();
    let report = set_up_fresh(&mut machine, &mut acl);

    assert_eq!(machine.calls, fresh_calls());
    let names: Vec<&str> = report.pool.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(
        names,
        ["ClauseoSb01", "ClauseoSb02", "ClauseoSb03", "ClauseoSb04"]
    );
    assert_eq!(report.made, report.pool);
    assert_eq!(report.reset, []);
    assert!(report.group_made);
    let pool: Vec<Sid> = (1001..=1004).map(sid).collect();
    let group = sid(1000);
    assert_eq!(
        machine.state.record,
        Some(MachineRecord {
            owner: owner(),
            group: group.clone(),
            pool: pool.clone(),
            data_dir: machine.data_dir(),
            removing: false,
        })
    );
    for user in &report.pool {
        let account = &machine.state.accounts[&user.name];
        assert!(account.hidden, "{}", user.name);
        assert!(account.groups.contains(&group), "{}", user.name);
        assert_eq!(
            machine.state.secrets[&user.sid], account.password,
            "{}",
            user.name
        );
        assert!(acl.has(&local_low(&machine, &user.name), &group, Rights::DenyWrite));
    }
    assert!(acl.has(&machine.device_sync(), &group, Rights::DenyWrite));
    let marker = machine.state.marker.clone().unwrap();
    assert_eq!(
        (marker.user_sid, marker.group_sid, marker.pool),
        (owner(), group, pool)
    );
    assert_eq!(
        report.journals,
        Ok(vec![(VOLUME.into(), JournalOutcome::Grown)])
    );
    assert_eq!(machine.journals[&PathBuf::from(VOLUME)], Some(JOURNAL_MAX));
    assert_eq!(report.fenced, None, "no matter is registered");
}

#[test]
fn a_large_journal_is_left_and_a_failed_one_never_fails_the_set_up() {
    let mut machine = FakeMachine::new("setup-journal");
    machine.journals.insert(VOLUME.into(), Some(1 << 30));
    machine.journals.insert("D:".into(), None);
    machine.fail("grow_journal D:");
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    assert_eq!(
        report.journals.unwrap(),
        [
            (VOLUME.into(), JournalOutcome::Left(1 << 30)),
            (
                "D:".into(),
                JournalOutcome::Failed("injected: grow_journal D:".to_owned())
            ),
        ]
    );
}

#[test]
fn secondary_logon_disabled_makes_no_account() {
    let mut machine = FakeMachine::new("setup-seclogon");
    machine.seclogon = ServiceStart::Disabled;
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::SecondaryLogon);
    assert_eq!(failure.cause, SECLOGON_DISABLED);
    assert_eq!(
        (failure.exit, failure.rolled_back),
        (SetupExit::Failed, true)
    );
    assert_eq!(machine.calls, ["lock", "read_record", "seclogon", "unlock"]);
    assert_eq!(machine.state, before);
}

#[test]
fn a_refused_logon_right_makes_no_account() {
    let mut machine = FakeMachine::new("setup-logon-right");
    machine.logon_right = LogonRight::Refused;
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.cause.as_str()),
        (Step::LogonRight, LOGON_REFUSED)
    );
    assert_eq!(
        machine.calls,
        ["lock", "read_record", "seclogon", "logon_right", "unlock"]
    );
    assert_eq!(machine.state, before);
}

#[test]
fn a_logon_type_not_granted_names_the_right_and_rolls_back() {
    let mut machine = FakeMachine::new("setup-1385");
    machine.fails.push((
        "log_on_once ClauseoSb01".to_owned(),
        MachineError::Os {
            call: "LogonUserW",
            code: ERROR_LOGON_TYPE_NOT_GRANTED,
        },
    ));
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.cause.as_str()),
        (Step::LogOn, LOGON_NOT_GRANTED)
    );
    assert!(failure.rolled_back);
    assert_eq!(machine.state, before);
}

#[test]
fn another_owner_refuses_with_the_owners_sid() {
    let mut machine = FakeMachine::new("setup-other-owner");
    machine.state.record = Some(MachineRecord {
        owner: Sid::parse(OTHER_OWNER).unwrap(),
        group: sid(900),
        pool: vec![sid(901)],
        data_dir: "/elsewhere".into(),
        removing: false,
    });
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (Step::Owner, SetupExit::OtherOwner)
    );
    assert!(failure.cause.contains(OTHER_OWNER), "{}", failure.cause);
    assert_eq!(machine.calls, ["lock", "read_record", "unlock"]);
    assert_eq!(machine.state, before);
}

#[test]
fn a_held_lock_exits_lock_held_and_touches_nothing() {
    let mut machine = FakeMachine::new("setup-lock");
    machine.lock_held = true;
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (Step::Lock, SetupExit::LockHeld)
    );
    assert_eq!(machine.calls, ["lock"]);
}

#[test]
fn the_owner_sid_never_ends_up_in_pool_sids() {
    // Account creation answering the owner's SID: the record write refuses it.
    let mut machine = FakeMachine::new("setup-owner-sid");
    machine.owner_sid_for = Some("ClauseoSb02".to_owned());
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::WriteRecord);
    assert!(failure.cause.contains(OWNER), "{}", failure.cause);
    assert_ne!(machine.written, []);
    for record in &machine.written {
        assert!(!record.pool.contains(&owner()), "{record:?}");
    }
    assert_eq!(machine.state.record, None, "rolled back to no record");
    let account = &machine.state.accounts[OWNER_NAME];
    assert_eq!(account.password, "the owner's");

    // A record that already lists the owner refuses before any change: no password reset.
    let mut machine = FakeMachine::new("setup-owner-recorded");
    machine.state.record = Some(MachineRecord {
        owner: owner(),
        group: sid(1000),
        pool: vec![owner()],
        data_dir: machine.data_dir(),
        removing: false,
    });
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Owner);
    assert_eq!(machine.calls, ["lock", "read_record", "unlock"]);
    assert_eq!(machine.state, before);
}

#[test]
fn a_recorded_sid_that_is_no_pool_account_refuses() {
    let mut machine = FakeMachine::new("setup-foreign-recorded");
    let foreign = Sid::parse("S-1-5-21-5-5-5-777").unwrap();
    machine.state.accounts.insert(
        "Administrator".to_owned(),
        fake::Account {
            sid: foreign.clone(),
            password: "admin".to_owned(),
            groups: std::collections::BTreeSet::default(),
            hidden: false,
        },
    );
    machine.state.record = Some(MachineRecord {
        owner: owner(),
        group: sid(1000),
        pool: vec![foreign],
        data_dir: machine.data_dir(),
        removing: false,
    });
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Pool);
    assert!(failure.cause.contains("Administrator"), "{}", failure.cause);
    assert_eq!(machine.state.accounts["Administrator"].password, "admin");
    assert!(!machine.calls.iter().any(|c| c.starts_with("set_password")));
}

#[test]
fn an_account_with_a_free_pool_name_is_skipped_never_adopted() {
    let mut machine = FakeMachine::new("setup-skip-name");
    let stranger = Sid::parse("S-1-5-21-5-5-5-555").unwrap();
    machine.state.accounts.insert(
        "ClauseoSb02".to_owned(),
        fake::Account {
            sid: stranger.clone(),
            password: "someone's".to_owned(),
            groups: std::collections::BTreeSet::default(),
            hidden: false,
        },
    );
    let report = set_up_fresh(&mut machine, &mut FakeAcl::default());
    let names: Vec<&str> = report.pool.iter().map(|u| u.name.as_str()).collect();
    assert_eq!(
        names,
        ["ClauseoSb01", "ClauseoSb03", "ClauseoSb04", "ClauseoSb05"]
    );
    assert_eq!(
        report.skipped_names,
        [("ClauseoSb02".to_owned(), stranger.clone())]
    );
    let account = &machine.state.accounts["ClauseoSb02"];
    assert_eq!(
        (account.password.as_str(), account.hidden),
        ("someone's", false)
    );
    assert!(account.groups.is_empty());
    assert!(!machine.state.record.unwrap().pool.contains(&stranger));
}

#[test]
fn a_temporary_profile_fails_the_set_up() {
    let mut machine = FakeMachine::new("setup-temp-profile");
    machine.temporary.insert("ClauseoSb03".to_owned());
    let before = machine.state.clone();
    let req = request(&machine, 0, Vec::new());
    let failure = set_up(&mut machine, &mut FakeAcl::default(), &req).unwrap_err();
    assert_eq!(failure.step, Step::Profile);
    assert!(failure.cause.contains("ClauseoSb03"), "{}", failure.cause);
    assert!(failure.rolled_back);
    assert_eq!(machine.state, before);
}

#[test]
fn exit_codes_round_trip() {
    let codes: Vec<u8> = SetupExit::ALL.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(
        codes,
        [0, 1, 2, 3, 4, 5],
        "appended from here on, never renumbered"
    );
    for exit in SetupExit::ALL {
        assert_eq!(SetupExit::from_code(u32::from(exit.as_code())), Some(exit));
    }
    for other in [
        6,
        77,
        255,
        ERROR_CANCELLED,
        ERROR_LOGON_TYPE_NOT_GRANTED,
        u32::MAX,
    ] {
        assert_eq!(SetupExit::from_code(other), None, "{other}");
    }
    assert_eq!(launch_error(ERROR_CANCELLED), SetupError::Refused);
    let SetupError::Failed(policy) = launch_error(ERROR_ACCESS_DISABLED_BY_POLICY) else {
        panic!("1260 is a failure");
    };
    assert!(
        policy.contains("policy") && policy.contains("1260"),
        "{policy}"
    );
    let SetupError::Failed(other) = launch_error(2) else {
        panic!("any other code is a failure");
    };
    assert!(other.contains("error 2"), "{other}");
}
