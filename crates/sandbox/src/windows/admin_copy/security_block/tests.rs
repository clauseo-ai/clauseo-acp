//! The security-block mark through a whole sequence on the fake: a start Windows security
//! refuses (225 or 226) is `ToolMissing` and records the mark; while it is there a missing copy
//! is `ToolMissing`, in the launch check, the start and the state; a copy put back and verified
//! deletes it, and with no mark a missing copy is `AdminCopyMissing` as before.

use std::path::Path;

use super::super::fake::Fake;
use super::super::{SETUP_EXE, admin_dir, check_launchable, protection_state};
use super::launch_recorded;
use crate::windows::setup::{ERROR_VIRUS_DELETED, ERROR_VIRUS_INFECTED, launch_error};
use crate::{Blocked, SetupError, SetupState};

/// A start on `fake` whose `ShellExecuteExW` fails with `code`.
fn start_failing(fake: &Fake, code: u32) -> Result<u32, SetupError> {
    launch_recorded(fake, |_exe: &Path| Err(launch_error(code)))
}

/// A start on `fake` that runs and exits 0.
fn start_running(fake: &Fake) -> Result<u32, SetupError> {
    launch_recorded(fake, |_exe: &Path| Ok(0))
}

/// The admin copy's path on the fake.
fn exe(fake: &Fake) -> std::path::PathBuf {
    admin_dir(&fake.program_files).join(SETUP_EXE)
}

#[test]
fn a_blocked_start_is_remembered_until_the_copy_is_back_and_verified() {
    for code in [ERROR_VIRUS_DELETED, ERROR_VIRUS_INFECTED] {
        let mut fake = Fake::ready();
        assert_eq!(
            start_failing(&fake, code),
            Err(SetupError::Blocked(Blocked::ToolMissing)),
            "{code}"
        );
        assert!(fake.security_mark.get(), "{code}: recorded");
        // Windows security deleted the copy: the next check says why, not "never installed".
        let (found, security) = fake.paths.remove(&key(&exe(&fake))).unwrap();
        assert_eq!(check_launchable(&fake), Err(Blocked::ToolMissing), "{code}");
        assert_eq!(
            start_running(&fake),
            Err(SetupError::Blocked(Blocked::ToolMissing)),
            "{code}"
        );
        assert_eq!(
            protection_state(&fake, SetupState::Ready, SetupState::Ready),
            SetupState::Blocked(Blocked::ToolMissing),
            "{code}"
        );
        assert!(
            fake.security_mark.get(),
            "{code}: kept while the copy is gone"
        );
        // Put back (the installer again, the copy allowed): verified, the mark goes, it runs.
        fake.set(&exe(&fake), found, security);
        assert!(check_launchable(&fake).is_ok(), "{code}");
        assert!(!fake.security_mark.get(), "{code}: cleared once verified");
        assert_eq!(start_running(&fake), Ok(0), "{code}");
    }
}

#[test]
fn a_start_that_runs_clears_the_mark_and_another_error_records_nothing() {
    let fake = Fake::ready();
    fake.security_mark.set(true);
    assert_eq!(start_running(&fake), Ok(0));
    assert!(!fake.security_mark.get(), "cleared");
    let Err(SetupError::Failed(why)) = start_failing(&fake, 5) else {
        panic!("5 is a failure");
    };
    assert!(why.contains("error 5"), "{why}");
    assert!(!fake.security_mark.get(), "another error records nothing");
}

#[test]
fn with_no_mark_a_missing_copy_is_admin_copy_missing_as_before() {
    let mut fake = Fake::ready();
    fake.paths.remove(&key(&exe(&fake)));
    assert_eq!(check_launchable(&fake), Err(Blocked::AdminCopyMissing));
    assert_eq!(
        start_running(&fake),
        Err(SetupError::Blocked(Blocked::AdminCopyMissing))
    );
    assert!(!fake.security_mark.get(), "a failed check records nothing");
    // The mark changes only a missing copy: a changed one is still AdminCopyChanged.
    let mut changed = Fake::ready();
    changed.security_mark.set(true);
    changed.hash = changed.hash.replace('0', "f");
    assert_eq!(check_launchable(&changed), Err(Blocked::AdminCopyChanged));
}

/// A path's text as the fake keys it.
fn key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}
