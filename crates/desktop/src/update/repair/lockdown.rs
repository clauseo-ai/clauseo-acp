//! The attempt's folder once its installer is written and sealed, and whether the admin copy is
//! really missing, on Windows.
//!
//! - **The lock-down** ([`lock_down`]): the installer's elevated step re-runs `$EXEPATH` from
//!   this folder (`admin_copy.nsh:405`), so nothing may be added beside it to be loaded by name.
//!   The folder's DACL becomes, protected (nothing inherited from `updates/`): this user read and
//!   execute on the folder plus `FILE_DELETE_CHILD` (so the start's sweep can still delete the
//!   installer), read and execute inherited by the file; `OWNER RIGHTS` (S-1-3-4) read and execute,
//!   on both, so being the owner restores neither write nor `WRITE_DAC`; SYSTEM full control.
//!   `SetNamedSecurityInfoW` carries the inherited entries to the file. The held handle is
//!   untouched: share modes don't govern `WRITE_DAC`, and access is checked only at open.
//! - **The admin copy** ([`admin_copy_missing`]): the record names no admin copy (or can't be
//!   read, which the launch check treats as missing too), or no plain file is at
//!   `<Program Files>\Clauseo\Sandbox\clauseo-sandbox-setup.exe`.

#![expect(
    unsafe_code,
    reason = "FFI to SetNamedSecurityInfoW with a converted descriptor"
)]

use std::io;
use std::path::Path;

use clauseo_settings::{KnownFolder, PlatformDirs as _, SystemDirs};
use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, PROTECTED_DACL_SECURITY_INFORMATION,
    PSECURITY_DESCRIPTOR,
};
use windows::core::HSTRING;

use super::plan;
use super::windows::own_sid;

/// The lock-down's DACL for `sid`: see the module's notes. `0x1200a9` is
/// `FILE_GENERIC_READ | FILE_GENERIC_EXECUTE`, `0x40` `FILE_DELETE_CHILD`.
pub(super) fn locked_sddl(sid: &str) -> String {
    format!(
        "D:P(A;;0x1200e9;;;{sid})(A;OIIO;0x1200a9;;;{sid})(A;OICI;0x1200a9;;;OW)(A;OICI;FA;;;SY)"
    )
}

/// Locks `folder` down (see the module's notes).
pub(super) fn lock_down(folder: &Path) -> io::Result<()> {
    let sddl = HSTRING::from(locked_sddl(&own_sid()?));
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live string and out-pointer; the descriptor is freed below.
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            &sddl,
            SDDL_REVISION_1,
            &raw mut descriptor,
            None,
        )
    }
    .map_err(io::Error::from)?;
    let set = set_dacl(folder, descriptor);
    // SAFETY: allocated by the conversion with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    set
}

/// Sets `folder`'s DACL to `descriptor`'s, protected.
fn set_dacl(folder: &Path, descriptor: PSECURITY_DESCRIPTOR) -> io::Result<()> {
    let (mut present, mut defaulted) = (windows::core::BOOL(0), windows::core::BOOL(0));
    let mut acl: *mut ACL = std::ptr::null_mut();
    // SAFETY: `descriptor` is valid; the out-pointers are live.
    unsafe {
        GetSecurityDescriptorDacl(
            descriptor,
            &raw mut present,
            &raw mut acl,
            &raw mut defaulted,
        )
    }
    .map_err(io::Error::from)?;
    // SAFETY: a live path; `acl` points into `descriptor`, alive for the call.
    let set = unsafe {
        SetNamedSecurityInfoW(
            &HSTRING::from(folder.as_os_str()),
            SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            None,
            Some(acl),
            None,
        )
    };
    if set.is_ok() {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(set.0.cast_signed()))
    }
}

/// Whether the admin copy is missing on this machine (see the module's notes).
pub(super) fn admin_copy_missing() -> bool {
    use clauseo_sandbox::windows::admin_copy::admin_exe;
    use clauseo_sandbox::windows::refresh::recorded_admin_copy_sha256;
    let recorded = recorded_admin_copy_sha256()
        .map(|hash| hash.is_some())
        .map_err(drop);
    let present = SystemDirs
        .known_folder(KnownFolder::ProgramFiles)
        .and_then(|program_files| std::fs::symlink_metadata(admin_exe(&program_files)).ok())
        .is_some_and(|meta| super::sweep::plain_file(&meta));
    plan::copy_missing(recorded, present)
}
