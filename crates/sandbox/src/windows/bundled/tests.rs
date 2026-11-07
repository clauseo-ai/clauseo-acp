//! The bundled exes' check, on every OS: a regular file is there; nothing, a folder or a link
//! in its place is `Blocked::ToolMissing`.

use super::*;
use crate::test_support::TempDir;

#[test]
fn every_bundled_exe_present_passes_and_one_missing_is_tool_missing() {
    let dir = TempDir::new("bundled-present");
    let bundle = fake_bundle(&dir);
    assert_eq!(check_bundled(&bundle), Ok(()));
    assert!(present(&bundle.join(SETUP_EXE)));
    crate::test_support::remove(&bundle.join(SETUP_EXE));
    assert_eq!(check_bundled(&bundle), Err(Blocked::ToolMissing));
    assert_eq!(
        check_bundled(&dir.path().join("no-such-folder")),
        Err(Blocked::ToolMissing)
    );
}

#[test]
fn the_setup_exe_is_bundled_by_its_name() {
    assert_eq!(SETUP_EXE, "clauseo-sandbox-setup.exe");
    assert!(BUNDLED_EXES.contains(&SETUP_EXE));
}

#[test]
fn a_folder_in_the_exes_place_is_missing() {
    let dir = TempDir::new("bundled-folder");
    dir.dir(&format!("app/{SETUP_EXE}"));
    assert_eq!(
        check_bundled(&dir.path().join("app")),
        Err(Blocked::ToolMissing)
    );
}

#[cfg(unix)]
#[test]
fn a_link_in_the_exes_place_is_missing_even_to_a_real_file() {
    let dir = TempDir::new("bundled-link");
    let real = dir.file("elsewhere/real.exe", "MZ");
    dir.dir("app");
    let link = dir.symlink(&format!("app/{SETUP_EXE}"), &real);
    assert!(!present(&link));
    assert_eq!(
        check_bundled(&dir.path().join("app")),
        Err(Blocked::ToolMissing)
    );
}

/// On Windows a file symlink needs admin or Developer Mode; without either the test can't make
/// one, and says so rather than pass on nothing.
#[cfg(windows)]
#[test]
fn a_link_in_the_exes_place_is_missing_even_to_a_real_file() {
    let dir = TempDir::new("bundled-link");
    let real = dir.file("elsewhere/real.exe", "MZ");
    dir.dir("app");
    let Ok(link) = dir.symlink_file(&format!("app/{SETUP_EXE}"), &real) else {
        eprintln!("skipped: this process may not make symlinks");
        return;
    };
    assert!(!present(&link));
    assert_eq!(
        check_bundled(&dir.path().join("app")),
        Err(Blocked::ToolMissing)
    );
}

#[test]
fn a_start_runs_only_once_the_exe_is_present() {
    let dir = TempDir::new("bundled-start");
    let missing = dir.path().join(SETUP_EXE);
    let mut started = false;
    let refused = start_present(&missing, || {
        started = true;
        Ok(0u32)
    });
    assert_eq!(refused, Err(SetupError::Blocked(Blocked::ToolMissing)));
    assert!(!started, "nothing starts without the exe");
    let exe = dir.file(SETUP_EXE, "MZ");
    assert_eq!(start_present(&exe, || Ok(7u32)), Ok(7));
}

/// Windows security can remove the exe between the check and the start: the start's 2 or 3
/// is `ToolMissing` too, and 1223 and 1260 stay as they were.
#[test]
fn a_start_that_finds_no_file_is_tool_missing() {
    use crate::windows::setup::{
        ERROR_ACCESS_DISABLED_BY_POLICY, ERROR_CANCELLED, ERROR_FILE_NOT_FOUND,
        ERROR_PATH_NOT_FOUND, launch_error,
    };
    let dir = TempDir::new("bundled-gone");
    let exe = dir.file(SETUP_EXE, "MZ");
    for code in [ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND] {
        let gone = start_present(&exe, || Err::<u32, _>(launch_error(code)));
        assert_eq!(
            gone,
            Err(SetupError::Blocked(Blocked::ToolMissing)),
            "{code}"
        );
    }
    assert_eq!(launch_error(ERROR_CANCELLED), SetupError::Refused);
    assert_eq!(
        launch_error(ERROR_ACCESS_DISABLED_BY_POLICY),
        SetupError::Blocked(Blocked::Policy)
    );
}
