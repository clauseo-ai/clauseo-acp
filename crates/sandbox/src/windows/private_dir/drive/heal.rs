//! [`Healer`] on NTFS, as `SYSTEM` (`set_aside/heal.rs`), and [`heal_drive_folders`], which
//! the refresh task calls. The drive folder is opened as itself (`FILE_FLAG_OPEN_REPARSE_POINT |
//! FILE_FLAG_BACKUP_SEMANTICS`, with the backup, restore and take-ownership privileges enabled,
//! so a squatter's DACL can't shut `SYSTEM` out), and everything happens on that handle:
//!
//! - its attributes and reparse tag (`GetFileInformationByHandleEx(FileAttributeTagInfo)`);
//! - its owner and DACL through `SetKernelObjectSecurity`, which writes the folder alone. Not
//!   `SetSecurityInfo`: it pushes inheritable entries down to what is already inside, which
//!   would walk the folder's contents. The DACL holds no inheritable entry either;
//! - its rename in place (`NtSetInformationFile(FileRenameInformation)`, no `RootDirectory`,
//!   a bare name, `ReplaceIfExists` false), which NT renames within the folder it is in. Not
//!   `SetFileInformationByHandle(FileRenameInfo)`: kernelbase turns a bare name into a path
//!   below the process's current folder.
//!
//! Each open and rename runs on a thread of its own and is waited for at most
//! [`CALL_WAIT`]: one a squatter's oplock or lease holds up past that is given up as `TimedOut`,
//! so the task goes on to the next folder. The thread keeps its own reference to the handle, so
//! the handle is closed only once the call returns, never under it; a rename that completes
//! after it was given up leaves the folder set aside with no fresh one, which the lawyer's next
//! launch makes. A rename refused because something holds the folder open (sharing, lock,
//! access denied, busy) is `ResourceBusy`, which the heal tries again.
//!
//! The lawyers are the record's `OwnerSid` and a per-user `AdminCopyOwner`, each checked to be a
//! SID; the volumes are the local fixed and removable drives (`GetLogicalDriveStringsW`). A
//! volume mounted on a folder, with no letter, is not healed (a known limit).

#![expect(
    unsafe_code,
    reason = "the drive folders' handles: privileges, drive types, attributes, rename by handle"
)]

use std::ffi::{OsStr, OsString};
use std::io;
use std::os::windows::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, SystemTime};

use windows::Wdk::Storage::FileSystem::{
    FILE_RENAME_INFORMATION, FileRenameInformation, NtSetInformationFile,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_NOT_ALL_ASSIGNED, GetLastError, HANDLE, LUID, RtlNtStatusToDosError,
};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LUID_AND_ATTRIBUTES, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
};
use windows::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_REPARSE_POINT, FILE_ATTRIBUTE_TAG_INFO,
    FILE_READ_ATTRIBUTES, FileAttributeTagInfo, GetDriveTypeW, GetFileInformationByHandleEx,
    GetLogicalDriveStringsW, READ_CONTROL, SYNCHRONIZE, WRITE_DAC, WRITE_OWNER,
};
use windows::Win32::System::IO::IO_STATUS_BLOCK;
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{PCWSTR, w};

use super::super::{Descriptor, Handle, PROTECTED_DACL, named};
use super::{drive_sddl, make_new, pool_group};
use crate::windows::record::{admin_copy_owner, sandbox_owner_sid};
use crate::windows::refresh::event_log;
use crate::windows::set_aside::heal::{CALL_WAIT, Facts, Healer, heal_all};
use crate::windows::sids::Sid;

/// `SYSTEM`, the owner a squatted folder is given.
const SYSTEM: &str = "S-1-5-18";
/// A squatted folder's DACL: `SYSTEM` and `Administrators`, protected, nothing inheritable.
const LOCKED_DOWN: &str = "D:P(A;;FA;;;SY)(A;;FA;;;BA)";
/// `GetDriveTypeW`'s answer for a removable drive (`DRIVE_REMOVABLE`, `WindowsProgramming`).
const DRIVE_REMOVABLE: u32 = 2;
/// `GetDriveTypeW`'s answer for a fixed drive (`DRIVE_FIXED`).
const DRIVE_FIXED: u32 = 3;
/// What the drive folder is opened with: read it, change its owner and DACL, rename it.
const ACCESS: u32 = READ_CONTROL.0
    | WRITE_DAC.0
    | WRITE_OWNER.0
    | DELETE.0
    | FILE_READ_ATTRIBUTES.0
    | SYNCHRONIZE.0;

/// Heals every lawyer's drive folder on every local drive (`set_aside::heal::heal_all`), as
/// `SYSTEM`, at `now`. A lawyer the record doesn't name, or privileges that can't be enabled,
/// leave nothing to do; the latter is logged.
pub(crate) fn heal_drive_folders(now: SystemTime) {
    let lawyers = lawyers();
    if lawyers.is_empty() {
        return;
    }
    if let Err(error) = enable_privileges() {
        let kind = error.kind();
        event_log(
            true,
            &format!("the drive folders weren't checked: no privileges ({kind})"),
        );
        return;
    }
    heal_all(&Ntfs, &drives(), &lawyers, now);
}

/// The record's `OwnerSid` and, for a per-user install, `AdminCopyOwner`: those that are SIDs,
/// once each.
fn lawyers() -> Vec<Sid> {
    let mut found: Vec<Sid> = Vec::new();
    let texts = [
        sandbox_owner_sid().ok().flatten(),
        admin_copy_owner().ok().flatten(),
    ];
    for text in texts.into_iter().flatten() {
        if let Ok(sid) = Sid::parse(&text)
            && !found.contains(&sid)
        {
            found.push(sid);
        }
    }
    found
}

/// The roots (`D:\`) of the local fixed and removable drives.
fn drives() -> Vec<PathBuf> {
    let mut buffer = vec![0_u16; 512];
    // SAFETY: `buffer` is writable for its length.
    let length = unsafe { GetLogicalDriveStringsW(Some(&mut buffer)) };
    let length = usize::try_from(length).unwrap_or(0);
    buffer.truncate(length.min(buffer.len()));
    buffer
        .split(|&unit| unit == 0)
        .filter(|root| !root.is_empty())
        .filter(|root| {
            let mut text = root.to_vec();
            text.push(0);
            // SAFETY: `text` is NUL-terminated and outlives the call.
            let kind = unsafe { GetDriveTypeW(PCWSTR(text.as_ptr())) };
            kind == DRIVE_FIXED || kind == DRIVE_REMOVABLE
        })
        .map(|root| PathBuf::from(String::from_utf16_lossy(root)))
        .collect()
}

/// Enables the backup, restore and take-ownership privileges in this process's token, which
/// `SYSTEM` holds; one not held is an error.
fn enable_privileges() -> io::Result<()> {
    let mut token = HANDLE::default();
    // SAFETY: a pseudo-handle and a live out-pointer; the handle is closed below.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &raw mut token) }
        .map_err(io::Error::other)?;
    let names = [
        w!("SeBackupPrivilege"),
        w!("SeRestorePrivilege"),
        w!("SeTakeOwnershipPrivilege"),
    ];
    let mut result = Ok(());
    for name in names {
        let mut luid = LUID::default();
        // SAFETY: a NUL-terminated literal and a live out-pointer.
        if let Err(error) = unsafe { LookupPrivilegeValueW(None, name, &raw mut luid) } {
            result = Err(io::Error::other(error));
            break;
        }
        let privileges = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        // SAFETY: the token was opened to adjust; `privileges` lives for the call.
        let adjusted = unsafe {
            AdjustTokenPrivileges(token, false, Some(&raw const privileges), 0, None, None)
        };
        // SAFETY: reads this thread's last error, which AdjustTokenPrivileges just set.
        let missing = unsafe { GetLastError() } == ERROR_NOT_ALL_ASSIGNED;
        if let Err(error) = adjusted {
            result = Err(io::Error::other(error));
            break;
        }
        if missing {
            result = Err(io::Error::from(io::ErrorKind::PermissionDenied));
            break;
        }
    }
    // SAFETY: the token was opened above and is closed once, here.
    drop(unsafe { CloseHandle(token) });
    result
}

/// The heal's calls on NTFS.
struct Ntfs;

/// A drive folder's handle, shared with the thread a bounded call runs on ([`bounded`]).
struct Shared(Handle);

// SAFETY: a kernel handle may be used and closed from any thread; `Shared` only reads it and
// closes it once, in `Handle`'s drop, when the last reference goes.
unsafe impl Send for Shared {}
// SAFETY: as for `Send`: the calls made through `&Shared` are kernel calls on the handle.
unsafe impl Sync for Shared {}

/// `call`'s answer, run on a thread of its own and waited for at most `wait`: one later is
/// `TimedOut`, and its answer, a handle included, is dropped on that thread when it comes.
fn bounded<T: Send + 'static>(
    wait: Duration,
    call: impl FnOnce() -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let (answer, answered) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("clauseo-heal-call".to_owned())
        .spawn(move || drop(answer.send(call())))?;
    match answered.recv_timeout(wait) {
        Ok(done) => done,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(io::Error::from(io::ErrorKind::TimedOut)),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(io::Error::other("the heal's call ended with no answer"))
        }
    }
}

/// `error` as `ResourceBusy` when it says something holds the folder open: a sharing or lock
/// violation, access denied (a folder with an item open inside), or busy.
fn busy(error: io::Error) -> io::Error {
    const HELD: [i32; 4] = [5, 32, 33, 170];
    if error
        .raw_os_error()
        .is_some_and(|code| HELD.contains(&code))
    {
        return io::Error::new(io::ErrorKind::ResourceBusy, error);
    }
    error
}

/// The pause between two tries of a rename held up (`set_aside::heal::BUSY_PAUSE`).
#[expect(
    clippy::disallowed_methods,
    reason = "a bounded pause in the SYSTEM task between two tries of a rename held up; the \
              tests pass a fake's (`Healer::pause`)"
)]
fn pause(wait: Duration) {
    thread::sleep(wait);
}

impl Healer for Ntfs {
    type Opened = Arc<Shared>;

    fn open(&self, path: &Path) -> io::Result<Option<Arc<Shared>>> {
        let path = path.to_owned();
        match bounded(CALL_WAIT, move || {
            Handle::open_for(&path, ACCESS).map(Shared)
        }) {
            Ok(handle) => Ok(Some(Arc::new(handle))),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    fn facts(&self, opened: &Arc<Shared>) -> io::Result<Facts> {
        let opened = &opened.0;
        let mut info = FILE_ATTRIBUTE_TAG_INFO::default();
        let size = u32::try_from(size_of::<FILE_ATTRIBUTE_TAG_INFO>()).map_err(io::Error::other)?;
        // SAFETY: the handle is open; `info` is writable for `size` bytes.
        unsafe {
            GetFileInformationByHandleEx(
                opened.0,
                FileAttributeTagInfo,
                (&raw mut info).cast(),
                size,
            )
        }
        .map_err(io::Error::other)?;
        Ok(Facts {
            reparse: info.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT.0 != 0
                || info.ReparseTag != 0,
            folder: info.FileAttributes & FILE_ATTRIBUTE_DIRECTORY.0 != 0,
            owner: opened.owner()?.as_str().to_owned(),
        })
    }

    fn lock_down(&self, opened: &Arc<Shared>) -> io::Result<()> {
        let opened = &opened.0;
        let system = Sid::parse(SYSTEM).map_err(io::Error::other)?;
        opened.set_owner(&system)?;
        opened.set(PROTECTED_DACL, &Descriptor::from_sddl(LOCKED_DOWN)?)
    }

    fn rename(&self, opened: &Arc<Shared>, name: &OsStr) -> io::Result<()> {
        let (shared, name): (Arc<Shared>, OsString) = (Arc::clone(opened), name.to_owned());
        bounded(CALL_WAIT, move || rename_in_place(&shared.0, &name)).map_err(busy)
    }

    fn make_fresh(&self, path: &Path, lawyer: &Sid) -> io::Result<()> {
        let group = pool_group()?;
        let dacl = drive_sddl(lawyer.as_str(), group.as_ref().map(Sid::as_str));
        let owned = format!("O:{}{dacl}", lawyer.as_str());
        if !make_new(path, &owned)? {
            return Err(io::Error::from(io::ErrorKind::AlreadyExists));
        }
        // Written again on the folder alone, so what is made inside inherits it
        // (`Descriptor::protected`), as `make_top` does.
        let handle = Handle::open_for(path, ACCESS)
            .map_err(|error| named(path, "can't be opened to secure", &error))?;
        if handle.owner()? != *lawyer {
            handle.set_owner(lawyer)?;
        }
        handle.set(PROTECTED_DACL, &Descriptor::protected(&dacl)?)
    }

    fn log(&self, error: bool, line: &str) {
        event_log(error, line);
    }

    fn random(&self) -> io::Result<u32> {
        let mut bytes = [0_u8; 4];
        getrandom::fill(&mut bytes).map_err(|error| io::Error::other(error.to_string()))?;
        Ok(u32::from_le_bytes(bytes))
    }

    fn pause(&self, wait: Duration) {
        pause(wait);
    }
}

/// Renames `opened` (with `DELETE`) to `name` in the folder it is in, never replacing.
fn rename_in_place(opened: &Handle, name: &OsStr) -> io::Result<()> {
    let name: Vec<u16> = name.encode_wide().collect();
    let header = std::mem::offset_of!(FILE_RENAME_INFORMATION, FileName);
    let name_bytes = name.len() * 2;
    let size = (header + name_bytes + 2).max(size_of::<FILE_RENAME_INFORMATION>());
    let length = u32::try_from(name_bytes).map_err(io::Error::other)?;
    let size_u32 = u32::try_from(size).map_err(io::Error::other)?;
    // u64 words, so the structure is aligned; zeroed, so `ReplaceIfExists` starts false, no
    // `RootDirectory` is given and the name ends in a NUL.
    let mut buffer = vec![0_u64; size.div_ceil(8)];
    let base = buffer.as_mut_ptr();
    let info = base.cast::<FILE_RENAME_INFORMATION>();
    let mut status = IO_STATUS_BLOCK::default();
    // SAFETY: `buffer` is at least `size_of::<FILE_RENAME_INFORMATION>()` bytes, aligned for it
    // and zeroed; the name's `name_bytes` bytes fit from `header` on, as `size` counts them;
    // the pointers stay inside `buffer`, which outlives the call; `status` is live; the handle
    // is open.
    let done = unsafe {
        (&raw mut (*info).FileNameLength).write(length);
        let start = base.cast::<u8>().add(header);
        std::ptr::copy_nonoverlapping(name.as_ptr().cast::<u8>(), start, name_bytes);
        NtSetInformationFile(
            opened.0,
            &raw mut status,
            base.cast_const().cast(),
            size_u32,
            FileRenameInformation,
        )
    };
    if done.is_err() {
        // SAFETY: RtlNtStatusToDosError only maps a number.
        let code = unsafe { RtlNtStatusToDosError(done) };
        return Err(io::Error::from_raw_os_error(
            i32::try_from(code).unwrap_or(i32::MAX),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
