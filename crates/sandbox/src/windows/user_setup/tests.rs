//! `UserSetup` on Windows CI, unelevated: a runner has no admin copy, so the state names it
//! and nothing starts; and the class key the start passes is the machine's.

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

/// The class key is opened read-only from `HKEY_LOCAL_MACHINE`, never `HKEY_CURRENT_USER` or
/// `HKEY_CLASSES_ROOT` (which merges the user's classes in), at `SOFTWARE\Classes\exefile`.
#[test]
fn the_class_key_comes_from_hklm_read_only() {
    let mut asked = None;
    let opened = exe_class_key(|root, subkey, access| {
        // SAFETY: `subkey` is the NUL-terminated literal `exe_class_key` passes.
        let subkey = unsafe { subkey.to_string() }.unwrap();
        asked = Some((root, subkey, access));
        Err(windows::Win32::Foundation::ERROR_FILE_NOT_FOUND)
    });
    assert!(
        matches!(opened, Err(SetupError::Failed(_))),
        "a key not opened starts nothing"
    );
    let (root, subkey, access) = asked.unwrap();
    assert_eq!(root, HKEY_LOCAL_MACHINE);
    assert_eq!(subkey, r"SOFTWARE\Classes\exefile");
    assert_eq!(access, KEY_READ);
    // The machine's key is there to open.
    let key = exe_class_key(open_key).unwrap();
    assert!(!key.0.is_invalid());
}

/// The start passes that key as the class key (`SEE_MASK_CLASSKEY`), so the verb is looked up
/// there alone, and keeps its other flags.
#[test]
fn the_start_looks_its_verb_up_in_the_class_key_alone() {
    let file: Vec<u16> = "x.exe\0".encode_utf16().collect();
    let parameters: Vec<u16> = "--remove\0".encode_utf16().collect();
    let key = exe_class_key(open_key).unwrap();
    let info = shell_info(&file, &parameters, key.0);
    assert_eq!(info.fMask & SEE_MASK_CLASSKEY, SEE_MASK_CLASSKEY);
    assert_eq!(info.hkeyClass, key.0);
    assert!(
        info.lpClass.is_null(),
        "no class name, which HKCU could supply"
    );
    assert_eq!(
        info.fMask & (SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI),
        SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI
    );
}
