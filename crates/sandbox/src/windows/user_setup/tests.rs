//! `UserSetup` on Windows CI, unelevated: the state it reads on a runner nothing set up, and
//! the admin copy that isn't there, so nothing starts.

use super::*;
use crate::Blocked;
use crate::test_support::TempDir;

#[test]
fn the_state_on_a_runner_nothing_set_up_is_needed() {
    let dir = TempDir::new("user-setup-state");
    let setup = UserSetup::new(dir.path().join("data"));
    assert_eq!(setup.state(), SetupState::Needed);
}

/// A runner has no admin copy and no recorded hash: every start is refused before
/// `ShellExecuteExW`, as `AdminCopyMissing`, recorded for `state` (the remove, which has no
/// result file, records nothing).
#[test]
fn with_no_admin_copy_nothing_starts_and_the_block_is_named() {
    let dir = TempDir::new("user-setup-no-admin-copy");
    let data = dir.dir("data");
    let setup = UserSetup::new(data);
    for outcome in [setup.run(), setup.extend(4), setup.remove()] {
        assert_eq!(
            outcome,
            Err(SetupError::Blocked(Blocked::AdminCopyMissing)),
            "no admin copy on a runner"
        );
    }
    assert_eq!(
        setup.state(),
        SetupState::Blocked(Blocked::AdminCopyMissing)
    );
}
