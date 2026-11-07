//! The Windows trust check on `ProgramData\Clauseo\policy.json` ([`crate::policy::trust`]'s
//! doc has the rule): the owner and DACL of the file and its folder, read from handles opened
//! without following a reparse point, and the file's link count; the file is then read through
//! the same handle.

#[cfg(test)]
mod tests;

use crate::policy::trust::{Distrust, Trust, parent_of};
use core::mem::offset_of;
use std::fs::File;
use std::io::{self, Read as _};
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::path::Path;
use std::time::Duration;
use windows::Win32::Foundation::{
    ERROR_FILE_NOT_FOUND, ERROR_LOCK_VIOLATION, ERROR_PATH_NOT_FOUND, ERROR_SHARING_VIOLATION,
    ERROR_SUCCESS, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, GetSecurityInfo, SE_FILE_OBJECT,
};
use windows::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, DACL_SECURITY_INFORMATION, GetAce, INHERIT_ONLY_ACE,
    OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
};
use windows::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, CreateFileW, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DELETE_CHILD, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_FLAGS_AND_ATTRIBUTES, FILE_READ_ATTRIBUTES,
    FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA,
    FILE_WRITE_EA, GetFileInformationByHandle, OPEN_EXISTING, READ_CONTROL, WRITE_DAC, WRITE_OWNER,
};
use windows::core::{HRESULT, HSTRING, Owned, PWSTR};

/// The owners trusted: BUILTIN\Administrators, SYSTEM and `TrustedInstaller`.
const TRUSTED: [&str; 3] = [
    "S-1-5-32-544",
    "S-1-5-18",
    "S-1-5-80-956008885-3418522649-1831038044-1853292631-2271478464",
];

/// CREATOR OWNER: an allow entry for it grants the owner, which must be trusted already.
const CREATOR_OWNER: &str = "S-1-3-0";

/// The rights no one but a trusted SID may hold on the file: every write-type right.
pub(super) const FILE_REFUSED: u32 = FILE_WRITE_DATA.0
    | FILE_APPEND_DATA.0
    | FILE_WRITE_EA.0
    | FILE_WRITE_ATTRIBUTES.0
    | DELETE.0
    | FILE_DELETE_CHILD.0
    | WRITE_DAC.0
    | WRITE_OWNER.0
    | GENERIC_WRITE.0
    | GENERIC_ALL.0;

/// The rights no one but a trusted SID may hold on the folder: those that remove, rename or
/// re-secure a file already in it. Adding a file (`FILE_ADD_FILE`) is allowed, since such a file
/// is its maker's and fails the owner check; so `ProgramData`'s default ACL passes.
pub(super) const FOLDER_REFUSED: u32 =
    FILE_DELETE_CHILD.0 | DELETE.0 | WRITE_DAC.0 | WRITE_OWNER.0 | GENERIC_WRITE.0 | GENERIC_ALL.0;

/// `ACCESS_ALLOWED_ACE_TYPE`.
const ALLOWED: u8 = 0;
/// The deny types (`ACCESS_DENIED`, `_OBJECT`, `_CALLBACK`, `_CALLBACK_OBJECT`): they only take
/// rights away, so they are skipped. Any other type of entry fails the check.
const DENIED: [u8; 4] = [1, 6, 10, 12];

/// Whether the `policy.json` at `file` may be read, with its bytes when it may: read through
/// the handle whose owner, DACL, attributes and link count were checked. The folder is opened
/// and judged first; a file that cannot be opened in it is ignored, since users may add files
/// to a trusted folder (the module doc).
pub(in crate::policy) fn trust(file: &Path) -> io::Result<Trust> {
    let folder_access = READ_CONTROL.0 | FILE_READ_ATTRIBUTES.0;
    let flags = FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS;
    let folder = match open(parent_of(file)?, folder_access, flags) {
        Ok(handle) => handle,
        Err(code) => return Ok(refused(code)),
    };
    if let Some(why) = judge(&folder, true, FOLDER_REFUSED)? {
        return Ok(Trust::Untrusted(why));
    }
    // `FILE_FLAG_BACKUP_SEMANTICS` lets a directory named `policy.json` open, to be ignored.
    // The folder's open above asks for no data access, so no one holding the folder can refuse
    // it with a sharing violation; the file's can be, and is retried (`busy`).
    let opened = retry_busy(|| open(file, GENERIC_READ.0, flags), pause);
    let policy = match opened {
        Ok(handle) => handle,
        Err(code) => return file_refused(code),
    };
    if let Some(why) = judge(&policy, false, FILE_REFUSED)? {
        return Ok(Trust::Untrusted(why));
    }
    if information(&policy)?.nNumberOfLinks != 1 {
        return Ok(Trust::Untrusted(Distrust::HardLink));
    }
    let mut bytes = Vec::new();
    File::from(policy).read_to_end(&mut bytes)?;
    Ok(Trust::Trusted(bytes))
}

/// How many times the file's open is tried while someone holds it without sharing, and how long
/// apart: an antivirus scan or IT's editor lets go within that.
pub(super) const BUSY_TRIES: usize = 3;
pub(super) const BUSY_PAUSE: Duration = Duration::from_millis(100);

/// Waits `pause` between two tries of a held file. Policy is read on the host's start-up thread
/// and on the setup thread's settings re-read, never on an async runtime's worker; the tests of
/// [`retry_busy`] pass a fake pause instead.
#[expect(
    clippy::disallowed_methods,
    reason = "Windows policy read: a short pause between retries of a sharing violation, on the start-up or setup thread, never async"
)]
fn pause(pause: Duration) {
    std::thread::sleep(pause);
}

/// Whether `code` says someone else holds the file: a sharing or lock violation.
fn busy(code: HRESULT) -> bool {
    code == ERROR_SHARING_VIOLATION.to_hresult() || code == ERROR_LOCK_VIOLATION.to_hresult()
}

/// `attempt`, tried up to [`BUSY_TRIES`] times with `sleep`([`BUSY_PAUSE`]) between while it
/// fails as [`busy`]; any other outcome at once. Policy is read on a start-up thread and on the
/// setup thread's settings re-read, never on an async runtime's worker.
pub(super) fn retry_busy<T>(
    mut attempt: impl FnMut() -> Result<T, HRESULT>,
    mut sleep: impl FnMut(Duration),
) -> Result<T, HRESULT> {
    let mut tries = 1;
    loop {
        match attempt() {
            Err(code) if busy(code) && tries < BUSY_TRIES => {
                sleep(BUSY_PAUSE);
                tries += 1;
            }
            outcome => return outcome,
        }
    }
}

/// What a failed open of the file in a trusted folder means. Held by someone else (after the
/// retries) is unreadable, failing closed: every user can read the file, so anyone could hold it
/// to switch the policy off for their own Clauseo. Otherwise as [`refused`].
pub(super) fn file_refused(code: HRESULT) -> io::Result<Trust> {
    if busy(code) {
        // Both codes are Win32 errors: the low 16 bits of the HRESULT are the error number.
        Err(io::Error::from_raw_os_error(code.0 & 0xFFFF))
    } else {
        Ok(refused(code))
    }
}

/// What an open that failed with `code` means: nothing there is absent; anything else (access
/// denied by a DACL someone planted where users may add files, …) is ignored, never unreadable.
pub(super) fn refused(code: HRESULT) -> Trust {
    if code == ERROR_FILE_NOT_FOUND.to_hresult() || code == ERROR_PATH_NOT_FOUND.to_hresult() {
        Trust::Missing
    } else {
        Trust::Untrusted(Distrust::Unusable)
    }
}

/// Why the object `handle` is open on is not trusted, if it is not: its kind, then its owner
/// and DACL with `refused` as the rights no other SID may hold.
fn judge(handle: &OwnedHandle, folder: bool, refused: u32) -> io::Result<Option<Distrust>> {
    if let Some(why) = kind(information(handle)?.dwFileAttributes, folder) {
        return Ok(Some(why));
    }
    Ok(Descriptor::of_handle(handle)?.distrust(refused))
}

/// Why an object with `attributes` is not trusted as a folder (`folder`) or a file, if it is
/// not: a reparse point, or the other kind.
pub(super) fn kind(attributes: u32, folder: bool) -> Option<Distrust> {
    if attributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0 {
        Some(Distrust::Link)
    } else if (attributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0) != folder {
        Some(Distrust::Unusable)
    } else {
        None
    }
}

/// A handle on `path` itself, opened for `access` with `flags` (a reparse point is opened,
/// not followed); the error's code when it does not open.
fn open(
    path: &Path,
    access: u32,
    flags: FILE_FLAGS_AND_ATTRIBUTES,
) -> Result<OwnedHandle, HRESULT> {
    // SAFETY: the name is a NUL-terminated wide string that outlives the call; no security
    // attributes and no template are passed.
    let opened = unsafe {
        CreateFileW(
            &HSTRING::from(path),
            access,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            None,
            OPEN_EXISTING,
            flags,
            None,
        )
    };
    match opened {
        // SAFETY: the handle was just opened and is valid; only the `OwnedHandle` closes it.
        Ok(handle) => Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) }),
        Err(error) => Err(error.code()),
    }
}

/// The attributes and link count of the open `handle`.
fn information(handle: &OwnedHandle) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: the handle is open, and `info` is a valid place for the answer.
    unsafe { GetFileInformationByHandle(HANDLE(handle.as_raw_handle()), &raw mut info) }?;
    Ok(info)
}

/// A security descriptor's owner and DACL, with the memory that holds them.
pub(super) struct Descriptor {
    /// The descriptor, freed on drop; `owner` and `dacl` point into it.
    _memory: Owned<HLOCAL>,
    owner: PSID,
    /// Null for a NULL DACL (or none), which grants everyone everything.
    dacl: *const ACL,
}

impl Descriptor {
    /// The owner and DACL of the object `handle` is open on.
    fn of_handle(handle: &OwnedHandle) -> io::Result<Descriptor> {
        let mut owner = PSID::default();
        let mut dacl: *mut ACL = core::ptr::null_mut();
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        // SAFETY: the handle is open with READ_CONTROL, and each out-pointer is a valid place.
        let got = unsafe {
            GetSecurityInfo(
                HANDLE(handle.as_raw_handle()),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                Some(&raw mut owner),
                None,
                Some(&raw mut dacl),
                None,
                Some(&raw mut descriptor),
            )
        };
        if got != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(got.0.cast_signed()));
        }
        // SAFETY: GetSecurityInfo allocated the descriptor with LocalAlloc; only `Owned` frees it.
        let memory = unsafe { Owned::new(HLOCAL(descriptor.0)) };
        Ok(Descriptor {
            _memory: memory,
            owner,
            dacl,
        })
    }

    /// Why the object is not trusted when no SID but a trusted one may hold `refused`: an owner
    /// that is not trusted, a NULL DACL, an allow entry (not inherit-only) giving another SID a
    /// refused right, or an entry of a type this check does not read.
    pub(super) fn distrust(&self, refused: u32) -> Option<Distrust> {
        if !sid_string(self.owner).is_some_and(|sid| TRUSTED.contains(&sid.as_str())) {
            return Some(Distrust::Owner);
        }
        if self.dacl.is_null() {
            return Some(Distrust::Writable);
        }
        // SAFETY: the DACL is a valid ACL inside the descriptor `self` holds.
        let count = unsafe { self.dacl.read_unaligned() }.AceCount;
        for index in 0..u32::from(count) {
            let mut ace: *mut core::ffi::c_void = core::ptr::null_mut();
            // SAFETY: the DACL is valid, `index` is below its entry count, and `ace` is a valid
            // place for the answer.
            if unsafe { GetAce(self.dacl, index, &raw mut ace) }.is_err() {
                return Some(Distrust::Writable);
            }
            // SAFETY: every entry starts with an ACE_HEADER.
            let header = unsafe { ace.cast::<ACE_HEADER>().read_unaligned() };
            if u32::from(header.AceFlags) & INHERIT_ONLY_ACE.0 != 0 {
                // It grants nothing on this object, only on what is made in it later.
                continue;
            }
            if DENIED.contains(&header.AceType) {
                continue;
            }
            if header.AceType != ALLOWED {
                return Some(Distrust::Writable);
            }
            // SAFETY: an entry of type ACCESS_ALLOWED_ACE_TYPE is an ACCESS_ALLOWED_ACE.
            let allowed = unsafe { ace.cast::<ACCESS_ALLOWED_ACE>().read_unaligned() };
            if allowed.Mask & refused == 0 {
                continue;
            }
            // SAFETY: its SID starts at `SidStart`, inside the entry.
            let sid = PSID(
                unsafe {
                    ace.cast::<u8>()
                        .add(offset_of!(ACCESS_ALLOWED_ACE, SidStart))
                }
                .cast(),
            );
            match sid_string(sid) {
                Some(sid) if TRUSTED.contains(&sid.as_str()) || sid == CREATOR_OWNER => {}
                _ => return Some(Distrust::Writable),
            }
        }
        None
    }
}

/// `sid` in its `S-1-…` form; `None` for no SID or one that does not convert.
fn sid_string(sid: PSID) -> Option<String> {
    if sid.is_invalid() {
        return None;
    }
    let mut text = PWSTR::null();
    // SAFETY: `sid` points at a SID inside a live descriptor, and `text` is a valid place for
    // the string the call allocates.
    unsafe { ConvertSidToStringSidW(sid, &raw mut text) }.ok()?;
    // SAFETY: `text` is the NUL-terminated wide string the call allocated, freed just below.
    let string = unsafe { text.to_string() }.ok();
    // SAFETY: `text` came from LocalAlloc and is freed once, here. A failed free leaves
    // nothing to do.
    let _freed = unsafe { LocalFree(Some(HLOCAL(text.0.cast()))) };
    string
}
