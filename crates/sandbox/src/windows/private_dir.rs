//! The sandbox folder's protected DACL: `<data_dir>\sandbox\` holds the matter SIDs, the set-up
//! marker and (S15 PR4) the sandbox user's sealed password, so only the desktop user, `SYSTEM`
//! and `Administrators` may open it, with nothing inherited from the data folder.
//!
//! A new folder is made with the DACL in place ([`create`]), so it is never open, even briefly.
//! A folder already there ([`secure_existing`]) must not be a link, must be owned by the user,
//! `SYSTEM` or `Administrators` (taken back when another account owns it and the user may, else
//! refused), gets the DACL again ([`protect`]), and each entry in it loses its explicit entries,
//! keeping only what it inherits from the folder ([`reset_to_inherited`]).
#![expect(
    unsafe_code,
    reason = "FFI to the security calls that set the folder's DACL"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::os::windows::fs::MetadataExt as _;
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HLOCAL, LocalFree, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
    SetNamedSecurityInfoW,
};
use windows::Win32::Security::{
    ACL, ACL_REVISION, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, InitializeAcl,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES, UNPROTECTED_DACL_SECURITY_INFORMATION,
};
use windows::Win32::Storage::FileSystem::{CreateDirectoryW, FILE_ATTRIBUTE_REPARSE_POINT};
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};

use super::record::current_user_sid;
use super::sids::Sid;

/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// The DACL for the desktop user `user`: full access, inherited by files and folders inside,
/// for `user`, `SYSTEM` and `Administrators`; `P` protects it from the parent's entries.
pub(crate) fn sddl(user: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
}

/// Whether `owner` may own the sandbox folder of the desktop user `user`.
pub(crate) fn owner_allowed(owner: &Sid, user: &Sid) -> bool {
    owner == user || owner.as_str() == SYSTEM || owner.as_str() == ADMINISTRATORS
}

/// Makes the folder `path` (its parent must exist) with [`sddl`]'s protected DACL for this
/// process's user in place from the start (`CreateDirectoryW` with the descriptor). `Ok(false)`
/// when something is there already, which this leaves to [`secure_existing`].
pub(crate) fn create(path: &Path) -> io::Result<bool> {
    let user = current_user_sid()?;
    let descriptor = Descriptor::from_sddl(&sddl(user.as_str()))?;
    let attributes = SECURITY_ATTRIBUTES {
        nLength: u32::try_from(size_of::<SECURITY_ATTRIBUTES>()).map_err(io::Error::other)?,
        lpSecurityDescriptor: descriptor.0.0,
        bInheritHandle: BOOL(0),
    };
    let target = wide(path);
    // SAFETY: `target` is NUL-terminated and outlives the call; `attributes` and the descriptor
    // it points to live until this function returns.
    match unsafe { CreateDirectoryW(PCWSTR(target.as_ptr()), Some(&raw const attributes)) } {
        Ok(()) => Ok(true),
        Err(error) if error.code() == ERROR_ALREADY_EXISTS.to_hresult() => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// Makes the folder already at `path` safe to keep records in: refuses a link or a file; when
/// its owner is not the user, `SYSTEM` or `Administrators`, takes ownership for the user (the
/// user owns the data folder) or refuses, naming the owner; sets the protected DACL again; and
/// resets every entry in it, links skipped, to its inherited entries only.
pub(crate) fn secure_existing(path: &Path) -> io::Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 || !metadata.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a link or not a folder", path.display()),
        ));
    }
    let user = current_user_sid()?;
    let found = owner(path)?;
    if !owner_allowed(&found, &user) {
        set_owner(path, &user).map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "{} is owned by {found}, not this user, and can't be taken back: {error}",
                    path.display()
                ),
            )
        })?;
    }
    protect(path)?;
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        let attributes = entry.metadata()?.file_attributes();
        if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 == 0 {
            reset_to_inherited(&entry.path())?;
        }
    }
    Ok(())
}

/// Gives the folder `path` the protected DACL of [`sddl`] for this process's user. Set every
/// time, which also puts it back if it was changed.
pub(crate) fn protect(path: &Path) -> io::Result<()> {
    let user = current_user_sid()?;
    let descriptor = Descriptor::from_sddl(&sddl(user.as_str()))?;
    let dacl = descriptor.dacl()?;
    set(
        path,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        None,
        Some(dacl),
    )
}

/// Drops every explicit entry of the file or folder `path` and unprotects its DACL, so it holds
/// only what it inherits from its folder.
pub(crate) fn reset_to_inherited(path: &Path) -> io::Result<()> {
    let mut empty = ACL::default();
    let size = u32::try_from(size_of::<ACL>()).map_err(io::Error::other)?;
    // SAFETY: `empty` is live, writable and `size` bytes long.
    unsafe { InitializeAcl(&raw mut empty, size, ACL_REVISION) }?;
    set(
        path,
        DACL_SECURITY_INFORMATION | UNPROTECTED_DACL_SECURITY_INFORMATION,
        None,
        Some(&raw const empty),
    )
}

/// The owner of `path`.
pub(crate) fn owner(path: &Path) -> io::Result<Sid> {
    let target = wide(path);
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: `target` is NUL-terminated; both out-pointers are live; the descriptor is freed by
    // `Descriptor` below, and `owner` points into it.
    let status = unsafe {
        GetNamedSecurityInfoW(
            PCWSTR(target.as_ptr()),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            &raw mut descriptor,
        )
    };
    let descriptor = Descriptor(descriptor);
    check(status)?;
    let mut text = PWSTR::null();
    // SAFETY: `owner` is a valid SID inside the live descriptor; `text` is a live out-pointer,
    // freed with LocalFree below.
    unsafe { ConvertSidToStringSidW(owner, &raw mut text) }?;
    // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
    let sid = unsafe { text.to_string() }.map_err(io::Error::other);
    // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
    unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    drop(descriptor);
    Sid::parse(&sid?).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Makes `sid` the owner of `path`. Needs `WRITE_OWNER` on it, and `sid` must be one the token
/// may set as owner (its user, or a group it holds as owner), unless it holds the restore
/// privilege.
pub(crate) fn set_owner(path: &Path, sid: &Sid) -> io::Result<()> {
    let mut parsed = PSID::default();
    // SAFETY: the HSTRING is NUL-terminated and lives for the call; `parsed` is a live
    // out-pointer, freed with LocalFree below.
    unsafe { ConvertStringSidToSidW(&HSTRING::from(sid.as_str()), &raw mut parsed) }?;
    let result = set(path, OWNER_SECURITY_INFORMATION, Some(parsed), None);
    // SAFETY: ConvertStringSidToSidW allocated the SID with LocalAlloc; freed once.
    unsafe { LocalFree(Some(HLOCAL(parsed.0))) };
    result
}

/// `SetNamedSecurityInfoW` on `path` with `info`, an owner and a DACL.
fn set(
    path: &Path,
    info: windows::Win32::Security::OBJECT_SECURITY_INFORMATION,
    owner: Option<PSID>,
    dacl: Option<*const ACL>,
) -> io::Result<()> {
    let target = wide(path);
    // SAFETY: `target` is NUL-terminated and outlives the call; the owner and DACL, when given,
    // are valid for the call (the callers keep them alive).
    check(unsafe {
        SetNamedSecurityInfoW(
            PCWSTR(target.as_ptr()),
            SE_FILE_OBJECT,
            info,
            owner,
            None,
            dacl,
            None,
        )
    })
}

/// A Win32 status as a result.
fn check(status: WIN32_ERROR) -> io::Result<()> {
    if status == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            i32::try_from(status.0).unwrap_or(i32::MAX),
        ))
    }
}

/// `path` as a NUL-terminated wide string.
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(once(0)).collect()
}

/// A security descriptor allocated with `LocalAlloc`, freed on drop.
struct Descriptor(PSECURITY_DESCRIPTOR);

impl Descriptor {
    /// The descriptor `sddl` describes.
    fn from_sddl(sddl: &str) -> io::Result<Descriptor> {
        let text: Vec<u16> = sddl.encode_utf16().chain(once(0)).collect();
        let mut descriptor = PSECURITY_DESCRIPTOR(null_mut());
        // SAFETY: `text` is NUL-terminated and outlives the call; `descriptor` is a live
        // out-pointer, and the descriptor it receives is freed on drop.
        unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                PCWSTR(text.as_ptr()),
                SDDL_REVISION_1,
                &raw mut descriptor,
                None,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Descriptor(descriptor))
    }

    /// Its DACL, which lives as long as `self`.
    fn dacl(&self) -> io::Result<*const ACL> {
        let mut present = BOOL(0);
        let mut dacl: *mut ACL = null_mut();
        let mut defaulted = BOOL(0);
        // SAFETY: the descriptor is valid while `self` lives; the out-pointers are live.
        unsafe {
            GetSecurityDescriptorDacl(self.0, &raw mut present, &raw mut dacl, &raw mut defaulted)
        }
        .map_err(io::Error::other)?;
        if !present.as_bool() || dacl.is_null() {
            return Err(io::Error::other("the sandbox folder's SDDL gave no DACL"));
        }
        Ok(dacl.cast_const())
    }
}

impl Drop for Descriptor {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            // SAFETY: allocated with LocalAlloc and freed here, once.
            unsafe { LocalFree(Some(HLOCAL(self.0.0))) };
        }
    }
}
