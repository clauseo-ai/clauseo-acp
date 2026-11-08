//! The result file and what a run of the exe means for the app: each exit code, with the file,
//! without it, unreadable or stale, and the launch errors (D-B4).

use super::*;
use crate::test_support::TempDir;
use crate::windows::setup::{
    APPROVER_LOGON_REFUSED, DATA_DIR_NOT_OWNED, ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED,
    ERROR_LOGON_TYPE_NOT_GRANTED, ERROR_VIRUS_DELETED, ERROR_VIRUS_INFECTED, LOGON_NOT_GRANTED,
    Step, launch_code, launch_error,
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

/// `read`'s result with its stamp checked (set, and no later than now) and taken off, to
/// compare with what was written.
fn unstamped(read: Result<Option<SetupResult>, String>) -> Result<Option<SetupResult>, String> {
    read.map(|result| {
        result.map(|result| {
            let at = result.at.expect("write stamps the time");
            assert!(at <= now_secs(), "{at}");
            SetupResult { at: None, ..result }
        })
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
    assert_eq!(unstamped(read(&data)), Ok(Some(first)));
    let text = std::fs::read_to_string(path(&data)).unwrap();
    assert!(text.contains("\"exit\": 3"), "{text}");
    assert!(text.contains("logging a sandbox user on"), "{text}");
    let done = SetupResult {
        exit: 0,
        step: None,
        cause: None,
        blocked: None,
        taken: None,
        code: None,
        kind: None,
        at: None,
    };
    write(&data, &done).unwrap();
    assert_eq!(unstamped(read(&data)), Ok(Some(done)));
    let text = std::fs::read_to_string(path(&data)).unwrap();
    let at = read(&data).unwrap().unwrap().at.unwrap();
    assert_eq!(text, format!("{{\n  \"exit\": 0,\n  \"at\": {at}\n}}\n"));
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
    // From the launch, 1385 is the approving account's sign-in, not the sandbox users' right.
    assert_eq!(
        launch_error(ERROR_LOGON_TYPE_NOT_GRANTED),
        SetupError::Failed(APPROVER_LOGON_REFUSED.to_owned())
    );
    assert!(!APPROVER_LOGON_REFUSED.contains("ClauseoSandboxUsers"));
    assert!(APPROVER_LOGON_REFUSED.contains("whoever looks after this computer"));
    let SetupError::Failed(other) = launch_error(2) else {
        panic!("2 is a failure");
    };
    assert!(other.contains("error 2"), "{other}");
    // Windows security refusing or removing the admin copy as it starts.
    for blocked in [ERROR_VIRUS_INFECTED, ERROR_VIRUS_DELETED] {
        assert_eq!(
            launch_error(blocked),
            SetupError::Blocked(Blocked::ToolMissing),
            "{blocked}"
        );
    }
}

#[test]
fn only_a_win32_hresult_is_read_as_its_win32_code() {
    // `HRESULT_FROM_WIN32` of 225 and 226, and the plain codes, read as themselves.
    for (hresult, code) in [
        (0x8007_00E1_u32, 225),
        (0x8007_00E2, 226),
        (0x8007_04C7, 1223),
        (225, 225),
    ] {
        assert_eq!(launch_code(hresult.cast_signed()), code, "{hresult:#x}");
    }
    assert_eq!(
        launch_error(launch_code(0x8007_00E2_u32.cast_signed())),
        SetupError::Blocked(Blocked::ToolMissing)
    );
    // Another facility's failure whose low bits are 225 or 2 is kept whole: never a block.
    for hresult in [0x8004_00E1_u32, 0x8000_4002, 0x8000_4003] {
        let code = launch_code(hresult.cast_signed());
        assert_eq!(code, hresult, "{hresult:#x}");
        assert!(
            matches!(launch_error(code), SetupError::Failed(_)),
            "{hresult:#x}"
        );
    }
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
    let blocked = Blocked::ToolMissing.to_string();
    assert!(
        blocked.contains("Windows security") && blocked.contains("clauseo-sandbox-setup.exe"),
        "{blocked}"
    );
    for words in [logon, policy, blocked] {
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
    let outdated = SetupState::Outdated {
        found: 1,
        current: 2,
    };
    assert_eq!(
        state_with(outdated, &last),
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
        taken: None,
        code: None,
        kind: None,
        at: None,
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

#[test]
fn a_stale_block_never_hides_another_owner_an_unfinished_removal_or_a_failure() {
    let blocked = Ok(Some(SetupResult {
        blocked: Some(Blocked::LogonRight),
        ..SetupResult::failed(SetupExit::Failed, "logging a sandbox user on", "1385")
    }));
    assert_eq!(
        state_with(SetupState::SetUpByOtherUser, &blocked),
        SetupState::SetUpByOtherUser
    );
    let removing = SetupState::Failed(
        "removal unfinished: reinstall and uninstall, or run set-up again".to_owned(),
    );
    assert_eq!(state_with(removing.clone(), &blocked), removing);
    let unreadable = SetupState::Failed("setup.json: bad".to_owned());
    assert_eq!(state_with(unreadable.clone(), &blocked), unreadable);
    assert_eq!(
        state_with(SetupState::Refused, &blocked),
        SetupState::Refused
    );
}

#[test]
fn a_data_folder_not_owned_by_the_user_has_its_own_exit_and_words() {
    assert_eq!(SetupExit::from_code(6), Some(SetupExit::DataDirNotOwned));
    for file in [None, Some(Ok(None)), Some(Err("missing".to_owned()))] {
        assert_eq!(
            outcome(6, file.as_ref()),
            Err(SetupError::Failed(DATA_DIR_NOT_OWNED.to_owned()))
        );
    }
    assert!(
        DATA_DIR_NOT_OWNED.contains("isn't owned by you"),
        "{DATA_DIR_NOT_OWNED}"
    );
    assert!(DATA_DIR_NOT_OWNED.contains("whoever looks after this computer"));
    // Exit 1 stays usage.
    let why = failed_with(outcome(1, Some(&Ok(None))));
    assert!(why.contains("started wrongly"), "{why}");
}

/// The read `state` uses never writes: a file that isn't this user's alone (on the tests'
/// stand-in, a mode open to the group or others) is read as missing and left exactly as it was;
/// once it is the user's alone, it reads.
#[cfg(unix)]
#[test]
fn the_state_read_writes_nothing_and_ignores_a_file_not_the_users_alone() {
    use std::os::unix::fs::PermissionsExt as _;

    let dir = TempDir::new("setup-result-unchanged");
    let data = dir.path().join("data");
    let blocked = SetupResult {
        blocked: Some(Blocked::Policy),
        ..SetupResult::failed(SetupExit::Failed, "starting the set-up program", "policy")
    };
    write(&data, &blocked).unwrap();
    let file = path(&data);
    crate::test_support::set_mode(&file, 0o644);
    let before = (
        std::fs::read(&file).unwrap(),
        std::fs::metadata(&file).unwrap().modified().unwrap(),
    );
    assert_eq!(read_unchanged(&data), Ok(None));
    assert_eq!(
        state_with(SetupState::Needed, &read_unchanged(&data)),
        SetupState::Needed
    );
    let after = (
        std::fs::read(&file).unwrap(),
        std::fs::metadata(&file).unwrap().modified().unwrap(),
    );
    assert_eq!(before, after);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o644, "nothing repaired");
    crate::test_support::set_mode(&file, 0o600);
    assert_eq!(unstamped(read_unchanged(&data)), Ok(Some(blocked)));
    assert_eq!(read_unchanged(&dir.path().join("none")), Ok(None));
}

/// Windows CI: the file `write` leaves is this user's alone, so the state's read takes it.
#[cfg(windows)]
#[test]
fn the_state_read_takes_the_file_write_left() {
    let dir = TempDir::new("setup-result-unchanged-windows");
    let data = dir.path().join("data");
    let told = SetupResult::failed(SetupExit::Failed, "x", "y");
    write(&data, &told).unwrap();
    assert_eq!(unstamped(read_unchanged(&data)), Ok(Some(told)));
}

#[test]
fn a_result_an_earlier_run_left_isnt_this_runs() {
    let stale = |exit: u8, at: Option<u64>| {
        Ok(Some(SetupResult {
            exit,
            step: None,
            cause: None,
            blocked: None,
            taken: None,
            code: None,
            kind: None,
            at,
        }))
    };
    // An earlier run's `{"exit":0}`, stamped before this run started, is no result.
    assert_eq!(of_run(stale(0, Some(99)), 100), Ok(None));
    // So the failure that left none is named by its code, never read as done.
    let why = failed_with(outcome(3, Some(&of_run(stale(3, Some(99)), 100))));
    assert!(why.contains("exit 3"), "{why}");
    assert!(why.contains("it left no result"), "{why}");
    // This run's is kept, and so is one an older exe wrote with no stamp.
    assert_eq!(of_run(stale(3, Some(100)), 100), stale(3, Some(100)));
    assert_eq!(of_run(stale(0, None), 100), stale(0, None));
    // Nothing to read, or a file that can't be, stays so.
    assert_eq!(of_run(Ok(None), 100), Ok(None));
    assert_eq!(of_run(Err("bad".to_owned()), 100), Err("bad".to_owned()));
}

#[test]
fn a_file_with_the_appended_fields_still_reads_and_one_without_them_too() {
    let dir = TempDir::new("setup-result-appended");
    let data = dir.path().join("data");
    dir.file(
        "data/sandbox/setup-result.json",
        r#"{"exit":3,"step":"lock","cause":"x","code":5,"kind":"win32","at":7,"later":1}"#,
    );
    let read = read(&data).unwrap().unwrap();
    assert_eq!(
        (
            read.exit,
            read.step.as_deref(),
            read.code,
            read.kind.as_deref(),
            read.at
        ),
        (3, Some("lock"), Some(5), Some("win32"), Some(7))
    );
    dir.file("data/sandbox/setup-result.json", r#"{"exit":0}"#);
    assert_eq!(super::read(&data).unwrap().unwrap().at, None);
}
