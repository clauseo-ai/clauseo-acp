//! `UserSetup` on Windows CI, unelevated: the state it reads on a runner nothing set up, and
//! a set-up exe that isn't there.

use super::*;
use crate::test_support::TempDir;

#[test]
fn the_state_on_a_runner_nothing_set_up_is_needed() {
    let dir = TempDir::new("user-setup-state");
    let setup = UserSetup::new(dir.path().join("data"), dir.path().join("none.exe"));
    assert_eq!(setup.state(), SetupState::Needed);
}

#[test]
fn a_missing_exe_fails_to_start_naming_the_error_and_leaves_no_result() {
    let dir = TempDir::new("user-setup-missing");
    let data = dir.path().join("data");
    let setup = UserSetup::new(data.clone(), dir.path().join("no-such-setup.exe"));
    for outcome in [setup.run(), setup.extend(4), setup.remove()] {
        let Err(SetupError::Failed(why)) = outcome else {
            panic!("a missing exe is a failure: {outcome:?}");
        };
        assert!(
            why.contains("didn't start") && why.contains("error"),
            "{why}"
        );
    }
    assert_eq!(result_file::read(&data), Ok(None));
}
