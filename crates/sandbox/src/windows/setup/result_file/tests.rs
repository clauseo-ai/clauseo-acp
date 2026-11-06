//! The result file and what a run of the exe means for the app: each exit code, with the file,
//! without it, unreadable or stale, and the launch errors (D-B4).

use super::*;
use crate::test_support::TempDir;
use crate::windows::setup::{
    ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED, ERROR_LOGON_TYPE_NOT_GRANTED,
    LOGON_NOT_GRANTED, Step, launch_error,
};

fn failure(step: Step, exit: SetupExit, cause: &str) -> Result<SetupReport, SetupFailure> {
    Err(SetupFailure {
        step,
        cause: cause.to_owned(),
        rolled_back: true,
        exit,
    })
}

fn failed_with(error: Result<(), SetupError>) -> String {
    match error {
        Err(SetupError::Failed(why)) => why,
        other => panic!("not a failure: {other:?}"),
    }
}

#[test]
fn the_file_round_trips_and_is_replaced_and_removed() {
    let dir = TempDir::new("setup-result-file");
    let data = dir.path().join("data");
    assert_eq!(read(&data), Ok(None));
    let first = SetupResult::of(&failure(Step::LogOn, SetupExit::Failed, "error 1326"));
    write(&data, &first).unwrap();
    assert_eq!(read(&data), Ok(Some(first)));
    let text = std::fs::read_to_string(path(&data)).unwrap();
    assert!(text.contains("\"exit\": 3"), "{text}");
    assert!(text.contains("logging a sandbox user on"), "{text}");
    let done = SetupResult {
        exit: 0,
        step: None,
        cause: None,
    };
    write(&data, &done).unwrap();
    assert_eq!(read(&data), Ok(Some(done)));
    assert_eq!(
        std::fs::read_to_string(path(&data)).unwrap(),
        "{\n  \"exit\": 0\n}\n"
    );
    remove(&data).unwrap();
    assert_eq!(read(&data), Ok(None));
    remove(&data).unwrap();
}

#[test]
fn a_file_that_isnt_a_result_is_unreadable() {
    let dir = TempDir::new("setup-result-bad");
    let data = dir.path().join("data");
    dir.file("data/sandbox/setup-result.json", "{\"exit\": \"three\"}");
    assert!(read(&data).is_err());
}

#[cfg(unix)]
#[test]
fn a_link_in_the_files_place_is_refused() {
    let dir = TempDir::new("setup-result-link");
    let data = dir.path().join("data");
    let elsewhere = dir.file("elsewhere.json", "{\"exit\": 0}");
    dir.dir("data/sandbox");
    dir.symlink("data/sandbox/setup-result.json", &elsewhere);
    assert!(read(&data).is_err());
    assert!(write(&data, &SetupResult::failed(SetupExit::Failed, "x", "y")).is_err());
    assert_eq!(std::fs::read(&elsewhere).unwrap(), b"{\"exit\": 0}");
}

#[test]
fn exit_0_is_done_whatever_the_file_says() {
    assert_eq!(outcome(0, None), Ok(()));
    assert_eq!(outcome(0, Some(&Ok(None))), Ok(()));
    let stale = SetupResult::failed(SetupExit::Failed, "x", "y");
    assert_eq!(outcome(0, Some(&Ok(Some(stale)))), Ok(()));
}

#[test]
fn a_failure_names_its_step_and_cause_from_the_file() {
    let told = SetupResult::of(&failure(Step::LogOn, SetupExit::Failed, LOGON_NOT_GRANTED));
    let why = failed_with(outcome(3, Some(&Ok(Some(told)))));
    assert!(why.contains("logging a sandbox user on"), "{why}");
    assert!(why.contains("Allow log on locally"), "{why}");
    assert!(why.contains("1385"), "{why}");

    let other = SetupResult::of(&failure(
        Step::Owner,
        SetupExit::OtherOwner,
        "S-1-5-21-1-2-3-1002",
    ));
    let why = failed_with(outcome(2, Some(&Ok(Some(other)))));
    assert!(why.contains("another Windows user"), "{why}");
    assert!(why.contains("S-1-5-21-1-2-3-1002"), "{why}");

    let why = failed_with(outcome(5, Some(&Ok(None))));
    assert!(why.contains("another set-up is running"), "{why}");
}

#[test]
fn a_missing_unreadable_or_stale_file_names_the_code() {
    let why = failed_with(outcome(3, Some(&Ok(None))));
    assert!(
        why.contains("exit 3") && why.contains("left no result"),
        "{why}"
    );
    let why = failed_with(outcome(3, Some(&Err("bad json".to_owned()))));
    assert!(why.contains("exit 3") && why.contains("bad json"), "{why}");
    // A file from another run (its code isn't this one) is not believed.
    let stale = SetupResult::failed(SetupExit::LockHeld, "taking the set-up lock", "held");
    let why = failed_with(outcome(3, Some(&Ok(Some(stale)))));
    assert!(why.contains("exit 3") && !why.contains("held"), "{why}");
    // A code no exe gives: a crash, or a newer exe.
    let why = failed_with(outcome(0xC000_0005, Some(&Ok(None))));
    assert!(why.contains(&0xC000_0005u32.to_string()), "{why}");
    let why = failed_with(outcome(1, None));
    assert!(
        why.contains("started wrongly") && why.contains("exit 1"),
        "{why}"
    );
    let why = failed_with(outcome(4, None));
    assert!(why.contains("part"), "{why}");
}

#[test]
fn the_launch_errors_map_as_d_b4_says() {
    assert_eq!(launch_error(ERROR_CANCELLED), SetupError::Refused);
    let SetupError::Failed(policy) = launch_error(ERROR_ACCESS_DISABLED_BY_POLICY) else {
        panic!("1260 is a failure");
    };
    assert!(
        policy.contains("policy") && policy.contains("1260"),
        "{policy}"
    );
    assert_eq!(
        launch_error(ERROR_LOGON_TYPE_NOT_GRANTED),
        SetupError::Failed(LOGON_NOT_GRANTED.to_owned())
    );
    let SetupError::Failed(other) = launch_error(2) else {
        panic!("2 is a failure");
    };
    assert!(other.contains("error 2"), "{other}");
}
