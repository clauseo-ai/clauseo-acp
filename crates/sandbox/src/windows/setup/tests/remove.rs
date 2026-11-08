//! `--remove` against the set-up's fake, on every OS: the exact order of a full removal, a
//! failure at each call leaving the rest to a re-run that finishes, a per-user removal by anyone
//! but the owner touching nothing, the lock's timeout, and a per-machine removal of the whole
//! pool, whoever asks.

use super::fake::{FakeAcl, FakeMachine, OTHER_OWNER, State};
use super::remove_fake::{Change, ParentToken, RemoveAcl, Remover};
use super::*;
use crate::windows::setup::remove::{acting_token, per_machine_allowed};

/// A machine a fresh set-up gave [`MIN_FREE_AFTER_RUN`] pool users (the group 1000, the users
/// 1001 to 1008), for `invoker`, and its writer.
pub(super) fn pool(test: &str, invoker: Option<Sid>) -> (Remover, RemoveAcl) {
    let mut machine = FakeMachine::new(test);
    let mut acl = FakeAcl::default();
    set_up_fresh(&mut machine, &mut acl);
    let remover = Remover::new(machine, invoker);
    let acl = remover.acl(acl);
    (remover, acl)
}

/// Another Windows user, an administrator, say.
fn other() -> Sid {
    Sid::parse(OTHER_OWNER).unwrap()
}

/// The calls of a full removal of [`pool`]'s eight users by its owner, with no ledger, in order.
fn full_removal() -> Vec<String> {
    let users = 1..=MIN_FREE_AFTER_RUN;
    let strays: Vec<String> = users.clone().map(|n| format!("100{n}")).collect();
    let mut lines: Vec<String> = [
        "read_record",
        "lock 60000",
        "read_record",
        "write_record removing",
    ]
    .map(str::to_owned)
    .to_vec();
    lines.push(format!("stop_strays {}", strays.join(",")));
    for n in users {
        let local_low = format!("Users/ClauseoSb0{n}/AppData/LocalLow");
        lines.extend([
            format!("account_name 100{n}"),
            format!("profile_of 100{n}"),
            format!("remove {local_low} 1000"),
            format!("walk {local_low}"),
            format!("remove {local_low} owner-rights"),
            format!("walk {local_low}"),
            format!("delete_profile 100{n}"),
            format!("delete_user ClauseoSb0{n}"),
        ]);
    }
    // Only once every pool account is gone and a second sweep ran (a stray must stay filtered).
    lines.push(format!("stop_strays {}", strays.join(",")));
    lines.push("remove_network".to_owned());
    lines.extend(
        [
            "program_data",
            "remove ProgramData/Microsoft/DeviceSync 1000",
            "walk ProgramData/Microsoft/DeviceSync",
            "owners_ledger",
            "group",
            "group_members 1000",
            "delete_group 1000",
            "delete_record",
            "unlock",
        ]
        .map(str::to_owned),
    );
    lines
}

/// Nothing set-up made is left: no pool account, profile, group, record or shared deny.
pub(super) fn assert_all_gone(machine: &Remover, acl: &RemoveAcl) {
    let state = &machine.m.state;
    assert_eq!(state.record, None);
    assert_eq!(state.group, None);
    assert!(state.profiles.is_empty(), "{:?}", state.profiles);
    let names: Vec<&String> = state.accounts.keys().collect();
    assert_eq!(names, [OWNER_NAME], "only the owner's account is left");
    assert!(acl.acl.aces.is_empty(), "{:?}", acl.acl.aces);
}

#[test]
fn a_full_removal_takes_each_user_off_in_the_uninstall_order_then_the_rest() {
    let (mut machine, mut acl) = pool("remove-order", Some(owner()));
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(machine.lines(), full_removal());
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(report.profiles, MIN_FREE_AFTER_RUN);
    assert_eq!(report.accounts, MIN_FREE_AFTER_RUN);
    assert!(report.group);
    assert_eq!(report.ledger, LedgerWork::Nothing);
    assert_all_gone(&machine, &acl);
    // The owner's files were read as the owner, and nothing else ran as the owner.
    assert!(machine.acted);
    assert!(
        machine.m.outside_owner.is_empty(),
        "{:?}",
        machine.m.outside_owner
    );
    assert!(
        acl.removes.iter().all(|(_, acting)| !acting),
        "{:?}",
        acl.removes
    );
}

/// The uninstaller's admin-copy teardown (`remove_then`) runs with the set-up lock still held:
/// after the record goes and before the unlock, so a refresh can't race the staging teardown.
#[test]
fn the_teardown_after_a_removal_runs_before_the_lock_is_given_back() {
    let (mut machine, mut acl) = pool("remove-then-locked", Some(owner()));
    let log = std::rc::Rc::clone(&machine.log);
    let mut teardown = |removed: &Removed| {
        assert!(matches!(removed, Removed::All(_)), "{removed:?}");
        log.borrow_mut().push("teardown".to_owned());
    };
    remove_then(&mut machine, &mut acl, false, false, &mut teardown).unwrap();
    let mut expected = full_removal();
    expected.insert(expected.len() - 1, "teardown".to_owned());
    assert_eq!(machine.lines(), expected);
}

#[test]
fn an_uninstall_disables_then_stops_the_set_up_task_before_it_waits_for_the_lock() {
    let (mut machine, mut acl) = pool("remove-uninstall-quiet", Some(owner()));
    remove_then(&mut machine, &mut acl, false, true, &mut |_| {}).unwrap();
    let mut expected = full_removal();
    let at = expected
        .iter()
        .position(|line| line == "lock 60000")
        .unwrap();
    expected.insert(at, "stop_set_up_task".to_owned());
    expected.insert(at, "disable_set_up_task".to_owned());
    assert_eq!(machine.lines(), expected);
    // Either failing is logged, and the lock is waited for as before.
    let (mut machine, mut acl) = pool("remove-uninstall-quiet-fails", Some(owner()));
    machine.fail_at.set(Some(1));
    remove_then(&mut machine, &mut acl, false, true, &mut |_| {}).unwrap();
    let lines = machine.lines();
    let first = [
        "read_record",
        "disable_set_up_task",
        "stop_set_up_task",
        "lock 60000",
    ];
    assert_eq!(lines[..4], first);
    // The app's own removal leaves the task alone.
    let (mut machine, mut acl) = pool("remove-app-keeps-task", Some(owner()));
    remove(&mut machine, &mut acl, false).unwrap();
    assert!(
        !machine
            .lines()
            .iter()
            .any(|line| line.contains("set_up_task"))
    );
}

/// A user whose profile has a reparse point on the way to `LocalLow` doesn't stop the removal:
/// its deny goes with the profile, which is deleted, and every other step runs as usual.
#[test]
fn a_profile_with_a_reparse_point_is_deleted_without_its_deny_and_the_removal_finishes() {
    let (mut machine, mut acl) = pool("remove-reparse", Some(owner()));
    machine.m.changed_profiles.insert("ClauseoSb03".to_owned());
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    let skipped = "Users/ClauseoSb03/AppData/LocalLow";
    let expected: Vec<String> = full_removal()
        .into_iter()
        .filter(|line| !line.ends_with(skipped) && !line.contains(&format!("{skipped} ")))
        .collect();
    assert_eq!(machine.lines(), expected);
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(report.profiles, MIN_FREE_AFTER_RUN);
    assert_eq!(report.accounts, MIN_FREE_AFTER_RUN);
    assert!(machine.m.state.profiles.is_empty());
}

#[test]
fn a_firewall_failure_doesnt_stop_the_removal() {
    let (mut machine, mut acl) = pool("remove-firewall", Some(owner()));
    machine.firewall_failure = Some("the firewall service is stopped");
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(machine.lines(), full_removal());
    let Removed::All(report) = removed else {
        panic!("{removed:?}");
    };
    assert_eq!(
        report.firewall.as_deref(),
        Some("the firewall service is stopped")
    );
    assert_all_gone(&machine, &acl);
}

#[test]
fn a_stray_that_wont_stop_keeps_the_network_on_and_ends_at_strays() {
    let (mut machine, mut acl) = pool("remove-stray", Some(owner()));
    machine.strays_left = 1;
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(failure.step, RemoveStep::Strays);
    assert_eq!(failure.exit, SetupExit::RemovedPartly);
    assert!(failure.cause.contains("wouldn't stop"), "{}", failure.cause);
    let lines = machine.lines();
    assert!(
        !lines.iter().any(|line| line == "remove_network"),
        "{lines:?}"
    );
    // The record stays, so the next run retries, and finishes once the stray is gone.
    assert!(machine.m.state.record.is_some());
    machine.strays_left = 0;
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    assert_all_gone(&machine, &acl);
}

#[test]
fn the_network_goes_only_after_every_account() {
    let lines = full_removal();
    let network = lines
        .iter()
        .position(|line| line == "remove_network")
        .unwrap();
    let last_user = lines
        .iter()
        .rposition(|line| line.starts_with("delete_user"))
        .unwrap();
    let strays = lines
        .iter()
        .position(|line| line.starts_with("stop_strays"))
        .unwrap();
    assert!(strays < last_user && last_user < network, "{lines:?}");
    // The second sweep is the call right before the network goes.
    assert!(lines[network - 1].starts_with("stop_strays"), "{lines:?}");
}

#[test]
fn a_failure_at_each_call_leaves_the_rest_to_the_next_run_which_finishes() {
    let full = full_removal();
    // Every call but the closing unlock, which can't fail.
    for at in 0..full.len() - 1 {
        let (mut machine, mut acl) = pool("remove-fail", Some(owner()));
        let before = machine.m.state.record.clone().unwrap();
        machine.fail_at.set(Some(at));
        let failure = remove(&mut machine, &mut acl, false).unwrap_err();
        let lines = machine.lines();
        // Nothing after the failed call, but giving the lock back once it was taken. Within one
        // target's denies every grant is still tried (`remove_shared_write_denies`), all but
        // the walk of a removal that failed.
        let mut expected: Vec<String> = full[..=at].to_vec();
        let denies = |line: &&String| line.starts_with("remove ") || line.starts_with("walk ");
        if denies(&&full[at]) {
            let mut rest = full[at + 1..].iter().take_while(denies);
            if full[at].starts_with("remove ") {
                rest.next();
            }
            expected.extend(rest.cloned());
        }
        if at >= 2 {
            expected.push("unlock".to_owned());
        }
        assert_eq!(lines, expected, "failing {}", full[at]);
        let (exit, step) = (failure.exit, failure.step);
        match at {
            0 | 2 => assert_eq!((exit, step), (SetupExit::Failed, RemoveStep::ReadRecord)),
            1 => assert_eq!((exit, step), (SetupExit::Failed, RemoveStep::Lock)),
            _ => assert_eq!(exit, SetupExit::RemovedPartly, "failing {}", full[at]),
        }
        // The record stays, naming every pool SID, with Removing set once it could be written.
        let record = machine.m.state.record.clone().unwrap();
        assert_eq!(record.pool, before.pool, "failing {}", full[at]);
        assert_eq!(record.removing, at > 3, "failing {}", full[at]);
        // The next run finishes, skipping what is gone.
        let removed = remove(&mut machine, &mut acl, false).unwrap();
        assert!(matches!(removed, Removed::All(_)), "after {}", full[at]);
        assert_all_gone(&machine, &acl);
    }
}

#[test]
fn the_steps_are_named_by_what_failed() {
    let full = full_removal();
    let step_of = |line: &str| match line.split(' ').next().unwrap() {
        "write_record" => RemoveStep::Flag,
        "stop_strays" => RemoveStep::Strays,
        "remove_network" => RemoveStep::Network,
        "account_name" | "delete_user" => RemoveStep::Account,
        "profile_of" | "delete_profile" => RemoveStep::Profile,
        "program_data" => RemoveStep::DeviceSync,
        "remove" | "walk" if line.contains("DeviceSync") => RemoveStep::DeviceSync,
        "remove" | "walk" => RemoveStep::LocalLow,
        "owners_ledger" => RemoveStep::Ledger,
        "group" | "group_members" | "delete_group" => RemoveStep::Group,
        "delete_record" => RemoveStep::Record,
        other => panic!("no step for {other}"),
    };
    for (at, line) in full.iter().enumerate().take(full.len() - 1).skip(3) {
        let (mut machine, mut acl) = pool("remove-steps", Some(owner()));
        machine.fail_at.set(Some(at));
        let failure = remove(&mut machine, &mut acl, false).unwrap_err();
        assert_eq!(failure.step, step_of(line), "failing {line}");
        assert!(!failure.cause.contains("S-1-"), "{}", failure.cause);
    }
}

#[test]
fn a_per_user_removal_by_anyone_but_the_owner_touches_nothing_and_exits_0() {
    let (mut machine, mut acl) = pool("remove-not-owner", Some(other()));
    let (state, aces): (State, _) = (machine.m.state.clone(), acl.acl.aces.clone());
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(removed, Removed::NotOwner);
    assert_eq!(machine.lines(), ["read_record"]);
    assert_eq!(machine.m.state, state);
    assert_eq!(acl.acl.aces, aces);
    assert!(!machine.acted);
}

#[test]
fn a_per_user_removal_with_no_one_found_who_asked_touches_nothing_and_keeps_the_records() {
    let (mut machine, mut acl) = pool("remove-no-parent", None);
    let state = machine.m.state.clone();
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Owner, SetupExit::Failed)
    );
    assert_ne!(failure.exit.as_code(), 0, "the hook keeps sandbox\\");
    assert_eq!(machine.lines(), ["read_record"]);
    assert_eq!(machine.m.state, state);
}

#[test]
fn nothing_set_up_exits_0_after_a_lock_taken_without_waiting_whoever_asks() {
    for (invoker, per_machine) in [(None, false), (None, true), (Some(other()), false)] {
        let machine = FakeMachine::new("remove-nothing");
        let mut machine = Remover::new(machine, invoker);
        let mut acl = machine.acl(FakeAcl::default());
        let removed = remove(&mut machine, &mut acl, per_machine).unwrap();
        assert_eq!(removed, Removed::NothingSetUp);
        assert_eq!(machine.lines(), ["read_record", "lock 0", "unlock"]);
    }
}

#[test]
fn no_record_while_a_set_up_holds_the_lock_is_lock_held_never_0() {
    let machine = FakeMachine::new("remove-nothing-locked");
    let mut machine = Remover::new(machine, Some(owner()));
    machine.lock_held = true;
    let mut acl = machine.acl(FakeAcl::default());
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Lock, SetupExit::LockHeld)
    );
    assert_eq!(machine.lines(), ["read_record", "lock 0"]);
}

#[test]
fn a_record_deleted_while_waiting_for_the_lock_is_nothing_to_remove() {
    let (mut machine, mut acl) = pool("remove-changed-gone", Some(owner()));
    machine.after_lock = Some(Change::Delete);
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(removed, Removed::NothingSetUp);
    let lines = ["read_record", "lock 60000", "read_record", "unlock"];
    assert_eq!(machine.lines(), lines);
}

#[test]
fn a_record_another_user_took_over_while_waiting_is_decided_again_and_left() {
    let (mut machine, mut acl) = pool("remove-changed-owner", Some(owner()));
    let mut taken = machine.m.state.record.clone().unwrap();
    taken.owner = other();
    machine.after_lock = Some(Change::Write(taken.clone()));
    let state_aces = acl.acl.aces.clone();
    let removed = remove(&mut machine, &mut acl, false).unwrap();
    assert_eq!(removed, Removed::NotOwner);
    let lines = ["read_record", "lock 60000", "read_record", "unlock"];
    assert_eq!(machine.lines(), lines);
    assert_eq!(machine.m.state.record, Some(taken));
    assert_eq!(acl.acl.aces, state_aces);
    assert!(!machine.acted);
}

#[test]
fn an_elevated_parent_whose_token_cant_be_lowered_takes_the_bounded_path_per_machine() {
    let (mut machine, mut acl) = pool("remove-elevated-parent", Some(owner()));
    machine.token = ParentToken::CantLower;
    let removed = remove(&mut machine, &mut acl, true).unwrap();
    assert!(matches!(removed, Removed::All(_)), "{removed:?}");
    // The administrator's bounded path: the ledger read as this process, nothing as the owner.
    assert!(!machine.acted);
    assert!(machine.lines().contains(&"admins_ledger".to_owned()));
    assert!(!machine.lines().contains(&"owners_ledger".to_owned()));
    assert_all_gone(&machine, &acl);
}

#[test]
fn an_elevated_parent_whose_token_cant_be_lowered_stops_a_per_user_run_touching_nothing() {
    let (mut machine, mut acl) = pool("remove-elevated-parent-user", Some(owner()));
    machine.token = ParentToken::CantLower;
    let (state, aces) = (machine.m.state.clone(), acl.acl.aces.clone());
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Owner, SetupExit::Failed)
    );
    assert_ne!(failure.exit.as_code(), 0, "the hook keeps sandbox\\");
    assert_eq!(machine.lines(), ["read_record"]);
    assert_eq!(machine.m.state, state, "the record and the pool are kept");
    assert_eq!(acl.acl.aces, aces);
    assert!(!machine.acted);
}

#[test]
fn the_owner_is_acted_as_only_with_a_standard_users_token() {
    #[derive(Debug, PartialEq)]
    struct Token {
        elevated: bool,
        lowered: bool,
    }
    let check = |token: &Token| Ok::<_, String>(token.elevated);
    let lower = |token: &Token| {
        assert!(token.elevated, "only an elevated token is lowered");
        Ok::<_, String>(Token {
            elevated: false,
            lowered: true,
        })
    };
    let standard = || Token {
        elevated: false,
        lowered: false,
    };
    let elevated = || Token {
        elevated: true,
        lowered: false,
    };
    // A standard parent's own token; an elevated parent's lowered copy, never itself.
    assert_eq!(acting_token(standard(), check, lower), Some(standard()));
    let lowered = acting_token(elevated(), check, lower).unwrap();
    assert!(lowered.lowered && !lowered.elevated);
    // A lowering or a check that fails gives no token: the administrator's path.
    let fails = |_: &Token| Err::<Token, _>("CreateRestrictedToken failed".to_owned());
    assert_eq!(acting_token(elevated(), check, fails), None);
    let unread = |_: &Token| Err::<bool, _>("GetTokenInformation failed".to_owned());
    assert_eq!(acting_token(standard(), unread, lower), None);
}

#[test]
fn per_machine_runs_only_elevated_or_as_system() {
    assert!(per_machine_allowed(true, false));
    assert!(per_machine_allowed(false, true));
    assert!(per_machine_allowed(true, true));
    assert!(!per_machine_allowed(false, false));
}

#[test]
fn a_lock_still_held_after_the_wait_exits_non_zero_and_touches_nothing() {
    let (mut machine, mut acl) = pool("remove-lock", Some(owner()));
    machine.lock_held = true;
    let state = machine.m.state.clone();
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Lock, SetupExit::LockHeld)
    );
    assert_ne!(failure.exit.as_code(), 0);
    assert_eq!(REMOVE_LOCK_WAIT_MS, 60_000, "half the hook's 120 s bound");
    assert_eq!(machine.lines(), ["read_record", "lock 60000"]);
    assert_eq!(machine.m.state, state);
}

#[test]
fn a_per_machine_removal_takes_the_whole_pool_off_whoever_asks() {
    for invoker in [Some(other()), None] {
        let (mut machine, mut acl) = pool("remove-per-machine", invoker.clone());
        let removed = remove(&mut machine, &mut acl, true).unwrap();
        let Removed::All(report) = removed else {
            panic!("{removed:?}");
        };
        assert_eq!((report.profiles, report.accounts), (8, 8));
        // No one to act as: the ledger is read as this process, and nothing ran as the owner.
        assert_eq!(report.ledger, LedgerWork::Nothing);
        assert!(!machine.acted, "{invoker:?}");
        let mut expected = full_removal();
        let at = expected.iter().position(|l| l == "owners_ledger").unwrap();
        expected[at] = "admins_ledger".to_owned();
        assert_eq!(machine.lines(), expected, "{invoker:?}");
        assert_all_gone(&machine, &acl);
    }
}

#[test]
fn a_per_machine_removal_by_the_owner_works_on_the_ledger_as_the_owner() {
    let (mut machine, mut acl) = pool("remove-per-machine-owner", Some(owner()));
    assert!(matches!(
        remove(&mut machine, &mut acl, true),
        Ok(Removed::All(_))
    ));
    assert_eq!(machine.lines(), full_removal());
    assert!(machine.acted);
}

#[test]
fn a_rerun_after_a_removal_left_removing_set_doesnt_write_it_again() {
    let (mut machine, mut acl) = pool("remove-flag", Some(owner()));
    let at = full_removal()
        .iter()
        .position(|l| l == "delete_group 1000")
        .unwrap();
    machine.fail_at.set(Some(at));
    remove(&mut machine, &mut acl, false).unwrap_err();
    let written = machine.m.written.len();
    let from = machine.lines().len();
    remove(&mut machine, &mut acl, false).unwrap();
    let rerun = machine.lines()[from..].to_vec();
    assert!(
        !rerun.iter().any(|l| l.starts_with("write_record")),
        "{rerun:?}"
    );
    assert_eq!(machine.m.written.len(), written);
    // What is gone is skipped: no profile, deny or account call for users already removed.
    assert!(
        !rerun.iter().any(|l| l.starts_with("delete_profile")),
        "{rerun:?}"
    );
    assert!(
        !rerun.iter().any(|l| l.starts_with("delete_user")),
        "{rerun:?}"
    );
    assert!(rerun.contains(&"delete_group 1000".to_owned()), "{rerun:?}");
}

#[test]
fn an_account_whose_name_isnt_a_pool_users_is_left_alone() {
    let (mut machine, mut acl) = pool("remove-other-name", Some(owner()));
    let account = machine.m.state.accounts.remove("ClauseoSb02").unwrap();
    machine.m.state.accounts.insert("alice".to_owned(), account);
    let failure = remove(&mut machine, &mut acl, false).unwrap_err();
    assert_eq!(
        (failure.step, failure.exit),
        (RemoveStep::Account, SetupExit::RemovedPartly)
    );
    assert!(machine.m.state.accounts.contains_key("alice"));
    assert!(!machine.lines().contains(&"delete_profile 1002".to_owned()));
}

#[test]
fn the_exit_codes_append_to_the_set_ups_and_removed_is_the_hooks_0() {
    assert_eq!(SetupExit::Removed.as_code(), 0);
    assert_eq!(SetupExit::Removed, SetupExit::Done);
    assert_eq!(SetupExit::RemovedPartly.as_code(), 4);
    assert_eq!(SetupExit::LockHeld.as_code(), 5);
    assert_eq!(SetupExit::AdminCopyLeft.as_code(), 10);
    let codes: Vec<u8> = SetupExit::ALL.iter().map(|exit| exit.as_code()).collect();
    assert_eq!(codes, (0..11).collect::<Vec<u8>>(), "no number used twice");
}
