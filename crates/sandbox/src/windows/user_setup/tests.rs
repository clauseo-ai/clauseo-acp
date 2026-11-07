//! `UserSetup` on Windows CI, unelevated: the state it reads on a runner nothing set up, and
//! a set-up exe that isn't there (`Blocked::ToolMissing`).

use super::*;
use crate::Blocked;
use crate::test_support::TempDir;

#[test]
fn the_state_on_a_runner_nothing_set_up_is_needed() {
    let dir = TempDir::new("user-setup-state");
    let setup = UserSetup::new(dir.path().join("data"), dir.path().join("none.exe")).unwrap();
    assert_eq!(setup.state(), SetupState::Needed);
}

#[test]
fn a_missing_exe_is_tool_missing_for_every_start_and_the_state_says_so() {
    let dir = TempDir::new("user-setup-missing");
    let data = dir.path().join("data");
    let setup = UserSetup::new(data, dir.path().join("no-such-setup.exe")).unwrap();
    for outcome in [
        setup.remove(),
        setup.reclaim(),
        setup.extend(4),
        setup.run(),
    ] {
        assert_eq!(outcome, Err(SetupError::Blocked(Blocked::ToolMissing)));
    }
    // A start through the result file records the block, so `state` reports it.
    assert_eq!(setup.state(), SetupState::Blocked(Blocked::ToolMissing));
}

#[test]
fn a_folder_in_the_exes_place_is_tool_missing() {
    let dir = TempDir::new("user-setup-folder");
    let exe = dir.dir("clauseo-sandbox-setup.exe");
    let setup = UserSetup::new(dir.path().join("data"), exe).unwrap();
    assert_eq!(
        setup.remove(),
        Err(SetupError::Blocked(Blocked::ToolMissing))
    );
}

#[test]
fn a_relative_exe_is_refused() {
    let dir = TempDir::new("user-setup-relative");
    for exe in [
        "clauseo-sandbox-setup.exe",
        r"bin\clauseo-sandbox-setup.exe",
        r"\setup.exe",
        "C:setup.exe",
    ] {
        let refused = UserSetup::new(dir.path().join("data"), PathBuf::from(exe));
        assert!(
            matches!(refused, Err(SetupError::Failed(_))),
            "{exe}: {refused:?}"
        );
    }
    let full = dir.path().join("clauseo-sandbox-setup.exe");
    assert!(UserSetup::new(dir.path().join("data"), full).is_ok());
}
