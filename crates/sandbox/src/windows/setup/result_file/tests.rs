//! The result file and what a run of the exe means for the app: each exit code, with the file,
//! without it, unreadable or stale, and the launch errors (D-B4).

use super::*;
use crate::test_support::TempDir;
use crate::windows::setup::{
    ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED, ERROR_LOGON_TYPE_NOT_GRANTED,
    LOGON_NOT_GRANTED, Step, launch_error,
};
use crate::{Blocked, SetupState};

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
        blocked: None,
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
fn a_file_over_the_cap_is_refused_unread() {
    let dir = TempDir::new("setup-result-cap");
    let data = dir.path().join("data");
    let padding = " ".repeat(usize::try_from(RESULT_CAP).unwrap());
    dir.file(
        "data/sandbox/setup-result.json",
        &format!("{{\"exit\": 0}}{padding}"),
    );
    let why = read(&data).unwrap_err();
    assert!(why.contains("larger than"), "{why}");
    // At the cap exactly, it reads.
    let fits = format!(
        "{{\"exit\": 0}}{}",
        " ".repeat(usize::try_from(RESULT_CAP).unwrap() - 11)
    );
    dir.file("data/sandbox/setup-result.json", &fits);
    assert_eq!(read(&data).unwrap().unwrap().exit, 0);
}

/// Windows CI: the file the exe writes has the sandbox folder's protected DACL, naming this
/// user, `SYSTEM` and `Administrators` only, and this user owns it.
#[cfg(windows)]
#[test]
fn the_written_file_is_the_owners_alone() {
    use crate::windows::record::current_user_sid;
    use crate::windows::test_acl::{entries, protected};

    let dir = TempDir::new("setup-result-dacl");
    let data = dir.path().join("data");
    write(&data, &SetupResult::failed(SetupExit::Failed, "x", "y")).unwrap();
    let file = path(&data);
    let me = current_user_sid().unwrap();
    assert!(protected(&file), "the DACL inherits nothing");
    let sids: Vec<String> = entries(&file)
        .into_iter()
        .map(|(.., sid)| sid.as_str().to_owned())
        .collect();
    assert!(sids.contains(&me.as_str().to_owned()), "{sids:?}");
    for sid in &sids {
        assert!(
            [me.as_str(), "S-1-5-18", "S-1-5-32-544"].contains(&sid.as_str()),
            "{sids:?}"
        );
    }
    assert_eq!(crate::windows::private_dir::owner(&file).unwrap(), me);
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
    let told = SetupResult::of(&failure(Step::LogOn, SetupExit::Failed, "error 1326"));
    let why = failed_with(outcome(3, Some(&Ok(Some(told)))));
    assert!(why.contains("logging a sandbox user on"), "{why}");
    assert!(why.contains("1326"), "{why}");

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
    assert_eq!(
        launch_error(ERROR_ACCESS_DISABLED_BY_POLICY),
        SetupError::Blocked(Blocked::Policy)
    );
    assert_eq!(
        launch_error(ERROR_LOGON_TYPE_NOT_GRANTED),
        SetupError::Blocked(Blocked::LogonRight)
    );
    let SetupError::Failed(other) = launch_error(2) else {
        panic!("2 is a failure");
    };
    assert!(other.contains("error 2"), "{other}");
}

#[test]
fn each_block_names_the_setting_for_it_and_no_path_or_sid() {
    let logon = Blocked::LogonRight.to_string();
    assert!(
        logon.contains("Allow log on locally") && logon.contains("ClauseoSandboxUsers"),
        "{logon}"
    );
    let policy = Blocked::Policy.to_string();
    assert!(
        policy.contains("policy") && policy.contains("clauseo-sandbox-setup.exe"),
        "{policy}"
    );
    for words in [logon, policy] {
        assert!(
            words.contains("whoever looks after this computer"),
            "{words}"
        );
        assert!(!words.contains(":\\") && !words.contains("S-1-"), "{words}");
    }
    let text = serde_json::to_string(&Blocked::LogonRight).unwrap();
    assert_eq!(text, "\"logon_right\"");
}

#[test]
fn a_logon_refused_with_1385_is_recorded_and_read_back_as_blocked() {
    let refused = failure(Step::LogOn, SetupExit::Failed, LOGON_NOT_GRANTED);
    let told = SetupResult::of(&refused);
    assert_eq!(told.blocked, Some(Blocked::LogonRight));
    // Left behind after a rollback that didn't finish: still the logon right.
    let partly = format!("{LOGON_NOT_GRANTED}; left behind: ClauseoSb01");
    let told_partly = SetupResult::of(&failure(Step::LogOn, SetupExit::Failed, &partly));
    assert_eq!(told_partly.blocked, Some(Blocked::LogonRight));
    // Any other failure isn't a block.
    let other = SetupResult::of(&failure(Step::LogOn, SetupExit::Failed, "error 1326"));
    assert_eq!(other.blocked, None);

    assert_eq!(
        outcome(3, Some(&Ok(Some(told.clone())))),
        Err(SetupError::Blocked(Blocked::LogonRight))
    );
    // A stale file (another exit) isn't believed, block or not.
    assert!(matches!(
        outcome(5, Some(&Ok(Some(told)))),
        Err(SetupError::Failed(_))
    ));
    assert!(matches!(
        outcome(3, Some(&Ok(Some(other)))),
        Err(SetupError::Failed(_))
    ));
}

#[test]
fn state_is_blocked_after_a_blocked_run_until_a_later_set_up_is_ready() {
    let dir = TempDir::new("setup-result-blocked-state");
    let data = dir.path().join("data");
    let blocked = SetupResult {
        blocked: Some(Blocked::Policy),
        ..SetupResult::failed(SetupExit::Failed, "starting the set-up program", "policy")
    };
    write(&data, &blocked).unwrap();
    let last = read(&data);
    assert_eq!(
        state_with(SetupState::Needed, &last),
        SetupState::Blocked(Blocked::Policy)
    );
    assert_eq!(
        state_with(SetupState::Failed("x".to_owned()), &last),
        SetupState::Blocked(Blocked::Policy)
    );
    // A Ready marker always wins.
    assert_eq!(state_with(SetupState::Ready, &last), SetupState::Ready);
    // A later run that succeeded writes a result with no block.
    let done = SetupResult {
        exit: 0,
        step: None,
        cause: None,
        blocked: None,
    };
    write(&data, &done).unwrap();
    assert_eq!(
        state_with(SetupState::Needed, &read(&data)),
        SetupState::Needed
    );
    // No result, or one that can't be read: the marker's state as it is.
    remove(&data).unwrap();
    assert_eq!(
        state_with(SetupState::Needed, &read(&data)),
        SetupState::Needed
    );
    assert_eq!(
        state_with(SetupState::Refused, &Err("bad".to_owned())),
        SetupState::Refused
    );
}
