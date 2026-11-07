//! `UserSetup` on Windows CI, unelevated: a runner has no admin copy, so the state names it
//! and nothing starts.

use super::*;
use crate::Blocked;
use crate::test_support::TempDir;

/// A runner has no admin copy: the state says so up front, before any set-up is tried.
#[test]
fn the_state_on_a_runner_with_no_admin_copy_is_admin_copy_missing() {
    let dir = TempDir::new("user-setup-state");
    let setup = UserSetup::new(dir.path().join("data"));
    assert_eq!(
        setup.state(),
        SetupState::Blocked(Blocked::AdminCopyMissing)
    );
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
