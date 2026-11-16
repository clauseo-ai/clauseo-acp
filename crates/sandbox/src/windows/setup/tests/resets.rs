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

/// A pool of eight, set up, with `marked` owed a profile reset and the entries on its `LocalLow`
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
        "profile_of 1001",
        "delete_profile 1001",
        "exists ClauseoSb01",
        "set_password ClauseoSb01",
        "log_on_once ClauseoSb01",
        "owner_of ClauseoSb01",
        "program_data",
        "protect_for_owner 1001,1009,1010,1011,1012",
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
    for n in 2..=8 {
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
    assert_eq!(report.reset.len(), 8, "a re-run resets every password");
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
    assert_eq!(rid(&report.made[0].sid), "1009");
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

/// Fails an extend over a pool owing `ClauseoSb02` a reset, after `fault` sets the machine up;
/// checks the step and that the mark stays, and returns the machine, its calls only the extend's,
/// and the failure's cause.
fn failing_reset(
    test: &str,
    step: Step,
    fault: impl FnOnce(&mut FakeMachine),
) -> (FakeMachine, String) {
    let (mut machine, mut acl) = pool_with_a_reset_owed(test, 1002);
    fault(&mut machine);
    let from = machine.calls.len();
    let req = request(&mut machine, 4, Vec::new());
    let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
    machine.calls.drain(..from);
    assert_eq!(
        (failure.step, failure.rolled_back),
        (step, true),
        "{}",
        failure.cause
    );
    assert!(machine.needs_reset.contains(&sid(1002)), "the mark stays");
    assert!(!machine.calls.iter().any(|c| c.starts_with("reset_done")));
    (machine, failure.cause)
}

#[test]
fn a_reset_must_prove_the_old_profile_is_gone() {
    // Its folder survives the delete: refused, named, before the password changes or a logon.
    let (machine, _) = failing_reset("setup-reset-stuck", Step::ResetProfile, |machine| {
        machine.faults.stuck.insert("ClauseoSb02".to_owned());
    });
    let calls = &machine.calls;
    assert!(!calls.iter().any(|c| c == "set_password ClauseoSb02"));
    assert!(!calls.iter().any(|c| c == "log_on_once ClauseoSb02"));

    // The logon makes it somewhere else: refused; the new password is saved by the rollback.
    let (machine, _) = failing_reset("setup-reset-moved", Step::ResetProfile, |machine| {
        machine.faults.moved.insert("ClauseoSb02".to_owned());
    });
    let now = &machine.state.accounts["ClauseoSb02"].password;
    assert_eq!(&machine.state.secrets[&sid(1002)], now);

    // Where the profile is can't be read: refused before the delete.
    let (machine, _) = failing_reset("setup-reset-unlisted", Step::ResetProfile, |machine| {
        machine.fail("profile_of 1002");
    });
    assert!(!machine.calls.iter().any(|c| c == "delete_profile 1002"));
}

/// The re-check's sequence: `ProfileList` lost the entry but `Users\ClauseoSb02` stayed. Run 1
/// refuses before the logon; run 2, over the `ClauseoSb02.PC` profile a logon would have made
/// beside it (as the previous code's run 1 did), deletes that, still finds the old folder, and
/// refuses again. The mark is set after both, and each failure names the folder left.
#[test]
fn a_folder_left_beside_the_profile_keeps_the_mark_across_runs() {
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-two-runs", 1002);
    let left = machine.state.profiles.remove(&sid(1002)).unwrap();
    for run in 1..=2 {
        if run == 2 {
            let beside = machine.dir.dir("Users/ClauseoSb02.PC/AppData/LocalLow");
            let profile = beside.parent().unwrap().parent().unwrap().to_owned();
            machine.state.profiles.insert(sid(1002), profile);
        }
        let from = machine.calls.len();
        let req = request(&mut machine, 4, Vec::new());
        let failure = set_up(&mut machine, &mut acl, &req).unwrap_err();
        assert_eq!(failure.step, Step::ResetProfile, "run {run}");
        assert!(
            failure.cause.contains(&left.display().to_string()),
            "{}",
            failure.cause
        );
        assert!(
            machine.needs_reset.contains(&sid(1002)),
            "run {run}: the mark stays"
        );
        let calls = machine.calls_since(from);
        assert!(
            !calls.contains(&"log_on_once ClauseoSb02"),
            "run {run}: {calls:?}"
        );
    }
}

/// A `<name>.*` folder of an earlier profile, in any case, refuses the reset and is named.
#[test]
fn a_stale_dotted_profile_folder_keeps_the_mark() {
    for stale in ["ClauseoSb02.OLD", "CLAUSEOSB02.000"] {
        let (machine, cause) = failing_reset("setup-reset-dotted", Step::ResetProfile, |machine| {
            machine.dir.dir(&format!("Users/{stale}"));
        });
        let folder = machine.dir.path().join("Users").join(stale);
        assert!(cause.contains(&folder.display().to_string()), "{cause}");
        // Where IT reads how to delete it without following a link in it, and what never to
        // use there.
        for words in [
            "\"Clearing a leftover Windows sandbox by hand\"",
            "never use takeown /R or icacls /T",
        ] {
            assert!(cause.contains(words), "{cause}");
        }
        assert!(machine.calls.iter().any(|c| c == "folder_names Users"));
        assert!(!machine.calls.iter().any(|c| c == "log_on_once ClauseoSb02"));
    }
    // A folder that only starts with the name is another account's: no refusal.
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-other", 1002);
    machine.dir.dir("Users/ClauseoSb021");
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.profiles_reset, Ok(vec![pool_user(2)]));
}

/// `GetUserProfileDirectoryW` may spell the folder in another case than `ProfileList`: the
/// same folder all the same.
#[test]
fn a_remade_profile_in_another_case_is_the_same_folder() {
    let (mut machine, mut acl) = pool_with_a_reset_owed("setup-reset-case", 1002);
    machine.faults.lower.insert("ClauseoSb02".to_owned());
    let req = request(&mut machine, 4, Vec::new());
    let report = set_up(&mut machine, &mut acl, &req).unwrap();
    assert_eq!(report.profiles_reset, Ok(vec![pool_user(2)]));
    assert!(machine.needs_reset.is_empty());
    let path = machine.state.profiles[&sid(1002)].display().to_string();
    assert!(path.ends_with("clauseosb02"), "{path}");
}

#[test]
fn a_failed_password_save_keeps_the_mark() {
    failing_reset("setup-reset-secrets", Step::Secrets, |machine| {
        machine.fail("protect_for_owner 1002,1009,1010,1011,1012");
    });
}
