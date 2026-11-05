//! The sandbox folder's protected DACL: `<data_dir>\sandbox\` holds the matter SIDs, the set-up
//! marker and (S15 PR4) the sandbox user's sealed password, so only the desktop user, `SYSTEM`
//! and `Administrators` may open it, with nothing inherited from the data folder.
//!
//! A new folder is made with the DACL in place ([`create`]). A folder already there
//! ([`secure_existing`]) and each record file the crate uses in it ([`check_file`]) are opened
//! once, without following a link, and everything after happens through that handle: what
//! [`repair_plan`] decides from the handle's facts, then the owner and the DACL.
#![expect(
    unsafe_code,
    reason = "FFI to the file and security calls on the sandbox folder"
)]

use std::io;
use std::iter::once;
use std::os::windows::ffi::OsStrExt as _;
use std::path::Path;
use std::ptr::null_mut;

use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HANDLE, HLOCAL, LocalFree, WIN32_ERROR,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
    ConvertStringSidToSidW, GetSecurityInfo, SDDL_REVISION_1, SE_FILE_OBJECT, SetSecurityInfo,
};
use windows::Win32::Security::{
    ACL, DACL_SECURITY_INFORMATION, GetSecurityDescriptorDacl, OBJECT_SECURITY_INFORMATION,
    OWNER_SECURITY_INFORMATION, PROTECTED_DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
    SECURITY_ATTRIBUTES,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    OPEN_EXISTING, ReOpenFile,
};
use windows::Win32::System::SystemServices::MAXIMUM_ALLOWED;
use windows::core::{BOOL, HSTRING, PCWSTR, PWSTR};

use super::record::current_user_sid;
use super::sids::Sid;
use super::write::{Found, repair_plan};

/// `SYSTEM`.
const SYSTEM: &str = "S-1-5-18";
/// `BUILTIN\Administrators`.
const ADMINISTRATORS: &str = "S-1-5-32-544";

/// The DACL for the desktop user `user`: full access, inherited by files and folders inside,
/// for `user`, `SYSTEM` and `Administrators`; `P` protects it from the parent's entries.
pub(crate) fn sddl(user: &str) -> String {
    format!("D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)")
}

/// A record file's DACL: the folder's three entries, written on the file itself and protected,
/// so it never depends on what the file would inherit.
pub(crate) fn file_sddl(user: &str) -> String {
    format!("D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)")
}

/// Whether `owner` may own the sandbox folder, or a file in it, of the desktop user `user`.
pub(crate) fn owner_allowed(owner: &Sid, user: &Sid) -> bool {
    owner == user || owner.as_str() == SYSTEM || owner.as_str() == ADMINISTRATORS
}

/// Makes the folder `path` (its parent must exist) with [`sddl`]'s protected DACL for this
/// process's user in place from the start (`CreateDirectoryW` with the descriptor). `Ok(false)`
/// when something is there already, which this leaves to [`protect`].
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
        Err(error) => Err(os_error(&error)),
    }
}

/// Secures the folder already at `path` through one handle: refused if it is a link or not a
/// folder; taken back for the user when another account owns it, or refused naming the owner;
/// then given the protected DACL again.
fn secure_existing(path: &Path) -> io::Result<()> {
    let user = current_user_sid()?;
    let handle = Handle::open(path)?;
    let handle = repair(path, handle, &user, true)?;
    let descriptor = Descriptor::from_sddl(&sddl(user.as_str()))?;
    let dacl = descriptor.dacl()?;
    handle
        .set(
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            Some(dacl),
        )
        .map_err(|error| named(path, "its DACL can't be set", &error))
}

/// Gives the folder `path` the protected DACL of [`sddl`], through a handle opened without
/// following a link; the same checks as [`secure_existing`].
pub(crate) fn protect(path: &Path) -> io::Result<()> {
    secure_existing(path)
}

/// Checks the record file `path` through one handle: missing is fine; refused if it is a link,
/// not a file, or has another hard link; taken back for the user when another account owns it,
/// or refused naming the owner; then given [`file_sddl`]'s protected DACL, the folder's three
/// entries written explicitly.
pub(crate) fn check_file(path: &Path) -> io::Result<()> {
    let user = current_user_sid()?;
    let handle = match Handle::open(path) {
        Ok(handle) => handle,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let handle = repair(path, handle, &user, false)?;
    let descriptor = Descriptor::from_sddl(&file_sddl(user.as_str()))?;
    let dacl = descriptor.dacl()?;
    handle
        .set(
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            None,
            Some(dacl),
        )
        .map_err(|error| named(path, "its DACL can't be set", &error))
}

#[cfg(test)]
/// The owner of `path`, read through a handle opened without following a link.
pub(crate) fn owner(path: &Path) -> io::Result<Sid> {
    Handle::open(path)?.owner()
}

#[cfg(test)]
/// Makes `sid` the owner of `path`, through a handle opened without following a link. Needs
/// `WRITE_OWNER`, and `sid` must be one the token may set as owner, unless it holds the restore
/// privilege.
pub(crate) fn set_owner(path: &Path, sid: &Sid) -> io::Result<()> {
    Handle::open(path)?.set_owner(sid)
}

/// Reads [`Found`] from `handle`, refuses what [`repair_plan`] refuses, and takes ownership back
/// for `user` when it says so. Returns a handle with the rights the owner now has.
fn repair(path: &Path, handle: Handle, user: &Sid, folder: bool) -> io::Result<Handle> {
    let info = handle.information()?;
    let owner = handle.owner()?;
    let found = Found {
        reparse: info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0,
        right_kind: (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) == folder,
        links: info.nNumberOfLinks,
        owner_allowed: owner_allowed(&owner, user),
    };
    let reown = repair_plan(found).map_err(|why| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} {why}", path.display()),
        )
    })?;
    if !reown {
        return Ok(handle);
    }
    handle.set_owner(user).map_err(|error| {
        let why = format!("is owned by {owner}, not this user, and can't be taken back");
        named(path, &why, &error)
    })?;
    // The rights were granted at the open, before the user owned it; open again for an owner's.
    handle.reopen()
}

/// `error` with what failed on `path`.
fn named(path: &Path, what: &str, error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), format!("{} {what}: {error}", path.display()))
}

/// A Win32 error as an `io::Error` whose kind follows its code (`NotFound` included).
fn os_error(error: &windows::core::Error) -> io::Error {
    io::Error::from_raw_os_error(error.code().0 & 0xFFFF)
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

/// The sharing every open here allows, so no other reader or writer is shut out.
fn share() -> windows::Win32::Storage::FileSystem::FILE_SHARE_MODE {
    FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
}

/// An open file or folder; closed on drop.
struct Handle(HANDLE);

impl Handle {
    /// Opens `path` itself, never what a link at it names (`FILE_FLAG_OPEN_REPARSE_POINT`), a
    /// folder too (`FILE_FLAG_BACKUP_SEMANTICS`), with the rights the caller is granted.
    fn open(path: &Path) -> io::Result<Handle> {
        let target = wide(path);
        // SAFETY: `target` is NUL-terminated and outlives the call; no security attributes or
        // template are passed, and the handle is owned by `Handle` from here.
        unsafe {
            CreateFileW(
                PCWSTR(target.as_ptr()),
                MAXIMUM_ALLOWED,
                share(),
                None,
                OPEN_EXISTING,
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                None,
            )
        }
        .map(Handle)
        .map_err(|error| os_error(&error))
    }

    /// The same file opened again through this handle, with the rights granted now.
    fn reopen(&self) -> io::Result<Handle> {
        // SAFETY: the handle is open; the new one is owned by `Handle` from here.
        unsafe {
            ReOpenFile(
                self.0,
                MAXIMUM_ALLOWED,
                share(),
                FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
            )
        }
        .map(Handle)
        .map_err(|error| os_error(&error))
    }

    /// Its attributes and link count.
    fn information(&self) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: the handle is open and `info` is a valid, writable struct for the call.
        unsafe { GetFileInformationByHandle(self.0, &raw mut info) }
            .map_err(|error| os_error(&error))?;
        Ok(info)
    }

    /// Its owner.
    fn owner(&self) -> io::Result<Sid> {
        let mut owner = PSID::default();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the handle is open; both out-pointers are live; `owner` points into the
        // descriptor, which `Descriptor` frees after the owner is read.
        let status = unsafe {
            GetSecurityInfo(
                self.0,
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                None,
                None,
                Some(&raw mut descriptor),
            )
        };
        let descriptor = Descriptor(descriptor);
        check(status)?;
        let mut text = PWSTR::null();
        // SAFETY: `owner` is a valid SID inside the live descriptor; `text` is a live
        // out-pointer, freed with LocalFree below.
        unsafe { ConvertSidToStringSidW(owner, &raw mut text) }.map_err(|e| os_error(&e))?;
        // SAFETY: ConvertSidToStringSidW wrote a NUL-terminated string.
        let sid = unsafe { text.to_string() }.map_err(io::Error::other);
        // SAFETY: allocated by ConvertSidToStringSidW with LocalAlloc, freed once.
        unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
        drop(descriptor);
        Sid::parse(&sid?).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
    }

    /// Makes `sid` its owner.
    fn set_owner(&self, sid: &Sid) -> io::Result<()> {
        let mut parsed = PSID::default();
        // SAFETY: the HSTRING is NUL-terminated and lives for the call; `parsed` is a live
        // out-pointer, freed with LocalFree below.
        unsafe { ConvertStringSidToSidW(&HSTRING::from(sid.as_str()), &raw mut parsed) }
            .map_err(|error| os_error(&error))?;
        let result = self.set(OWNER_SECURITY_INFORMATION, Some(parsed), None);
        // SAFETY: ConvertStringSidToSidW allocated the SID with LocalAlloc; freed once.
        unsafe { LocalFree(Some(HLOCAL(parsed.0))) };
        result
    }

    /// `SetSecurityInfo` with `info`, an owner and a DACL, which the caller keeps alive.
    fn set(
        &self,
        info: OBJECT_SECURITY_INFORMATION,
        owner: Option<PSID>,
        dacl: Option<*const ACL>,
    ) -> io::Result<()> {
        // SAFETY: the handle is open; the owner and DACL, when given, are valid for the call.
        check(unsafe { SetSecurityInfo(self.0, SE_FILE_OBJECT, info, owner, None, dacl, None) })
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from CreateFileW or ReOpenFile and is closed once, here. A
        // failed close leaves nothing to do.
        drop(unsafe { CloseHandle(self.0) });
    }
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
