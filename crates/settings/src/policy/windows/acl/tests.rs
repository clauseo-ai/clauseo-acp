//! The ACL walk on security descriptors built from SDDL. The whole check on a real folder is in
//! `tests/disk.rs`.

use super::{
    BUSY_PAUSE, BUSY_TRIES, Descriptor, FILE_REFUSED, FOLDER_REFUSED, file_refused, kind, refused,
    retry_busy,
};
use crate::policy::trust::{Distrust, Trust};
use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_LOCK_VIOLATION, ERROR_PATH_NOT_FOUND,
    ERROR_SHARING_VIOLATION, HLOCAL,
};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    ACL, GetSecurityDescriptorDacl, GetSecurityDescriptorOwner, PSECURITY_DESCRIPTOR, PSID,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_ARCHIVE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT,
};
use windows::core::{BOOL, HSTRING, Owned};

impl Descriptor {
    /// The descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> Descriptor {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the string outlives the call, and `descriptor` is a valid place for the
        // descriptor it allocates.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                &HSTRING::from(sddl),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .unwrap();
        // SAFETY: the call allocated it with LocalAlloc; only `Owned` frees it.
        let memory = unsafe { Owned::new(HLOCAL(descriptor.0)) };
        let (mut owner, mut defaulted) = (PSID::default(), BOOL::default());
        // SAFETY: the descriptor is valid, and the out-pointers are valid places.
        unsafe { GetSecurityDescriptorOwner(descriptor, &raw mut owner, &raw mut defaulted) }
            .unwrap();
        let (mut present, mut dacl): (BOOL, *mut ACL) = (BOOL::default(), core::ptr::null_mut());
        // SAFETY: as above.
        unsafe {
            GetSecurityDescriptorDacl(
                descriptor,
                &raw mut present,
                &raw mut dacl,
                &raw mut defaulted,
            )
        }
        .unwrap();
        Descriptor {
            _memory: memory,
            owner,
            dacl: if present.as_bool() {
                dacl
            } else {
                core::ptr::null_mut()
            },
        }
    }
}

/// The verdict on a file whose descriptor is `sddl`.
fn file(sddl: &str) -> Option<Distrust> {
    Descriptor::from_sddl(sddl).distrust(FILE_REFUSED)
}

/// The verdict on a folder whose descriptor is `sddl`.
fn folder(sddl: &str) -> Option<Distrust> {
    Descriptor::from_sddl(sddl).distrust(FOLDER_REFUSED)
}

/// What a folder made by an administrator under `ProgramData` inherits: Users may add files
/// and folders (`WD,AD,WEA,WA`, 0x116) on the folder itself.
const DEFAULT_FOLDER: &str = "O:BAG:SYD:(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICIIO;GA;;;CO)\
                              (A;OICI;0x1200a9;;;BU)(A;CI;0x116;;;BU)";

#[test]
fn an_admin_owned_file_only_admins_can_change_is_trusted() {
    assert_eq!(file("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)"), None);
    // Owned by SYSTEM or `TrustedInstaller` is trusted too.
    assert_eq!(file("O:SYG:SYD:(A;;FA;;;SY)(A;;FR;;;BU)"), None);
    assert_eq!(
        file("O:S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464G:SYD:(A;;FA;;;SY)"),
        None
    );
}

#[test]
fn a_file_users_can_write_is_not_trusted() {
    assert_eq!(
        file("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;FR;;;BU)(A;;FW;;;BU)"),
        Some(Distrust::Writable)
    );
    // Each write-type right alone is enough: append, attributes, delete, the DACL.
    for right in ["0x4", "0x100", "SD", "WD", "WO", "GW", "GA"] {
        assert_eq!(
            file(&format!("O:BAG:SYD:(A;;FA;;;BA)(A;;{right};;;BU)")),
            Some(Distrust::Writable),
            "{right}"
        );
    }
}

#[test]
fn a_file_users_can_write_data_or_extended_attributes_to_is_not_trusted() {
    // FILE_WRITE_DATA (0x2) alone, and FILE_WRITE_EA (0x10) alone.
    assert_eq!(
        file("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x2;;;BU)"),
        Some(Distrust::Writable)
    );
    assert_eq!(
        file("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x10;;;BU)"),
        Some(Distrust::Writable)
    );
}

#[test]
fn a_file_a_user_owns_is_not_trusted() {
    assert_eq!(
        file("O:S-1-5-21-1004336348-1177238915-682003330-1001G:SYD:(A;;FA;;;SY)(A;;FA;;;BA)"),
        Some(Distrust::Owner)
    );
}

#[test]
fn a_null_dacl_is_not_trusted() {
    assert_eq!(
        file("O:BAG:SYD:NO_ACCESS_CONTROL"),
        Some(Distrust::Writable)
    );
    assert_eq!(
        folder("O:BAG:SYD:NO_ACCESS_CONTROL"),
        Some(Distrust::Writable)
    );
}

#[test]
fn creator_owner_full_control_with_an_admin_owner_is_trusted() {
    // Inherit-only, as Windows writes it, and as a plain entry.
    assert_eq!(
        folder("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;BA)(A;OICIIO;FA;;;CO)"),
        None
    );
    assert_eq!(file("O:BAG:SYD:(A;;FA;;;SY)(A;;FA;;;CO)"), None);
}

#[test]
fn an_inherit_only_entry_grants_nothing_on_the_object() {
    assert_eq!(file("O:BAG:SYD:(A;;FA;;;BA)(A;OICIIO;FA;;;BU)"), None);
}

#[test]
fn an_entry_of_a_type_the_check_does_not_read_fails_closed() {
    // An allow-object entry, even for reading only. It names an object type: without one,
    // Windows' SDDL parser writes `OA` as a plain allow entry (seen on windows-2025).
    assert_eq!(
        file("O:BAG:SYD:(A;;FA;;;BA)(OA;;FR;bf967aba-0de6-11d0-a285-00aa003049e2;;BU)"),
        Some(Distrust::Writable)
    );
    // A deny entry only takes rights away.
    assert_eq!(file("O:BAG:SYD:(A;;FA;;;BA)(D;;FA;;;BU)"), None);
}

#[test]
fn the_default_program_data_folder_acl_is_trusted_for_the_folder_not_for_the_file() {
    assert_eq!(folder(DEFAULT_FOLDER), None);
    // The file's rule is strict: the same ACL on the file lets Users write it.
    assert_eq!(file(DEFAULT_FOLDER), Some(Distrust::Writable));
}

#[test]
fn a_folder_that_lets_users_remove_or_resecure_a_file_is_not_trusted() {
    for right in ["0x40", "SD", "WD", "WO", "GW", "GA"] {
        assert_eq!(
            folder(&format!("O:BAG:SYD:(A;;FA;;;BA)(A;;{right};;;BU)")),
            Some(Distrust::Writable),
            "{right}"
        );
    }
    assert_eq!(
        folder("O:S-1-5-21-1004336348-1177238915-682003330-1001G:SYD:(A;;FA;;;BA)"),
        Some(Distrust::Owner)
    );
}

#[test]
fn a_file_or_folder_that_does_not_open_is_absent_only_when_nothing_is_there() {
    assert_eq!(refused(ERROR_FILE_NOT_FOUND.to_hresult()), Trust::Missing);
    assert_eq!(refused(ERROR_PATH_NOT_FOUND.to_hresult()), Trust::Missing);
    // A deny-all DACL a user planted on a file they added: ignored, not unreadable, so other
    // users' security keys do not lock.
    assert_eq!(
        refused(ERROR_ACCESS_DENIED.to_hresult()),
        Trust::Untrusted(Distrust::Unusable)
    );
    assert_eq!(
        file_refused(ERROR_ACCESS_DENIED.to_hresult()).unwrap(),
        Trust::Untrusted(Distrust::Unusable)
    );
    assert_eq!(
        file_refused(ERROR_FILE_NOT_FOUND.to_hresult()).unwrap(),
        Trust::Missing
    );
}

#[test]
fn a_file_someone_holds_without_sharing_is_retried_then_unreadable_not_ignored() {
    // Any user can read the file, so any user could hold it to switch the policy off.
    for code in [ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION] {
        let (mut tries, mut pauses) = (0, Vec::new());
        let outcome = retry_busy(
            || -> Result<(), _> {
                tries += 1;
                Err(code.to_hresult())
            },
            |pause| pauses.push(pause),
        );
        assert_eq!(outcome, Err(code.to_hresult()));
        assert_eq!(tries, BUSY_TRIES);
        assert_eq!(pauses, vec![BUSY_PAUSE; BUSY_TRIES - 1]);
        let error = file_refused(code.to_hresult()).unwrap_err();
        assert_eq!(error.raw_os_error(), Some(code.0.cast_signed()), "{error}");
    }
}

#[test]
fn a_held_file_let_go_within_the_retries_is_opened_and_other_failures_are_not_retried() {
    let mut tries = 0;
    let outcome = retry_busy(
        || {
            tries += 1;
            if tries < BUSY_TRIES {
                Err(ERROR_SHARING_VIOLATION.to_hresult())
            } else {
                Ok("opened")
            }
        },
        |_| {},
    );
    assert_eq!(outcome, Ok("opened"));
    let mut denied = 0;
    let outcome = retry_busy(
        || -> Result<(), _> {
            denied += 1;
            Err(ERROR_ACCESS_DENIED.to_hresult())
        },
        |_| {},
    );
    assert_eq!(outcome, Err(ERROR_ACCESS_DENIED.to_hresult()));
    assert_eq!(denied, 1);
}

#[test]
fn a_directory_named_policy_json_or_a_reparse_point_is_not_trusted() {
    let (file, dir, link) = (
        FILE_ATTRIBUTE_ARCHIVE.0,
        FILE_ATTRIBUTE_DIRECTORY.0,
        FILE_ATTRIBUTE_REPARSE_POINT.0,
    );
    assert_eq!(kind(file, false), None);
    assert_eq!(kind(dir, true), None);
    assert_eq!(kind(dir, false), Some(Distrust::Unusable));
    assert_eq!(kind(file, true), Some(Distrust::Unusable));
    assert_eq!(kind(file | link, false), Some(Distrust::Link));
    assert_eq!(kind(dir | link, true), Some(Distrust::Link));
}

#[test]
fn the_folder_the_installer_creates_is_trusted_and_its_files_inherit_a_trusted_acl() {
    // The per-machine installer's SDDL for `ProgramData\Clauseo` (#592): protected, SYSTEM and
    // Administrators full, Users read and execute. A file made in it inherits the same entries.
    let installer = "O:BAG:BAD:PAI(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)(A;OICI;0x1200a9;;;BU)";
    assert_eq!(folder(installer), None);
    assert_eq!(
        file("O:BAG:BAD:AI(A;ID;FA;;;SY)(A;ID;FA;;;BA)(A;ID;0x1200a9;;;BU)"),
        None
    );
}

#[test]
fn a_win32_error_keeps_its_win32_code_and_others_keep_their_hresult() {
    use windows::core::{Error, HRESULT};

    let denied = super::win32_io_error(Error::from_hresult(HRESULT::from_win32(5)));
    assert_eq!(denied.raw_os_error(), Some(5));
    assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);

    let missing = super::win32_io_error(Error::from_hresult(HRESULT::from_win32(2)));
    assert_eq!(missing.raw_os_error(), Some(2));
    assert_eq!(missing.kind(), std::io::ErrorKind::NotFound);

    let e_fail = HRESULT(0x8000_4005_u32.cast_signed());
    let other = super::win32_io_error(Error::from_hresult(e_fail));
    assert_eq!(other.raw_os_error(), Some(e_fail.0));
}
