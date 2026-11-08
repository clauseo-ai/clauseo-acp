//! The client's effects on Windows: the task, the published name and its key's owner, the pipe
//! opened and its server's facts, the overlapped reads, the Authz check and the clock.
#![expect(
    unsafe_code,
    reason = "FFI to the registry, named pipe and process calls"
)]

use std::io;
use std::path::Path;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use windows::Win32::Foundation::{
    ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_IO_PENDING, ERROR_OPERATION_ABORTED,
    ERROR_PIPE_BUSY, GENERIC_READ, HANDLE, HLOCAL, LocalFree, NTSTATUS, UNICODE_STRING,
    WAIT_OBJECT_0, WAIT_TIMEOUT,
};
use windows::Win32::Security::Authorization::{
    GetSecurityInfo, SE_KERNEL_OBJECT, SE_OBJECT_TYPE, SE_REGISTRY_KEY,
};
use windows::Win32::Security::{OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_FLAG_OVERLAPPED, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_NONE, OPEN_EXISTING,
    QueryDosDeviceW, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT,
};
use windows::Win32::System::IO::CancelIoEx;
use windows::Win32::System::Pipes::{GetNamedPipeServerProcessId, GetNamedPipeServerSessionId};
use windows::Win32::System::Registry::{
    HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, RRF_RT_REG_SZ, RegCloseKey, RegGetValueW,
    RegOpenKeyExW,
};
use windows::Win32::System::Threading::{Sleep, WaitForSingleObject};
use windows::core::{HSTRING, PCWSTR, PWSTR};

use super::verify::{ServerFacts, check_server};
use super::{ConnectError, POLICY_KEY, POLICY_VALUE, Pipe, StartError, Watch, policy_says_off};
use crate::windows::acl::authz::{self, Checker};
use crate::windows::acl::object::{Handle, os_error};
use crate::windows::admin_copy::{self, os as admin_os};
use crate::windows::pipe::os::{Overlapped, ended_code};
use crate::windows::record::read_dword;
use crate::windows::refresh::{TaskError, last_watch_result, request_watch};
use crate::windows::setup::stop::windows_stopper::listed_users;
use crate::windows::setup::watch::protocol::{PIPE_VALUE, WATCH_KEY};
use crate::windows::sids::Sid;
use crate::windows::token::SYSTEM;
use crate::windows::token::os::sid_text;
use crate::{Access, Clock, SystemClock};

/// `SystemProcessIdInformation`: a process's image name by its id, with no handle to it.
const PROCESS_ID_INFORMATION: u32 = 88;
/// `STATUS_ACCESS_DENIED`.
const STATUS_ACCESS_DENIED: NTSTATUS = NTSTATUS(0xC000_0022_u32.cast_signed());

/// `NtQuerySystemInformation`, which the `windows` crate's features in use don't bind.
mod nt {
    windows::core::link!("ntdll.dll" "system" fn NtQuerySystemInformation(
        class: u32,
        information: *mut core::ffi::c_void,
        length: u32,
        returned: *mut u32
    ) -> windows::Win32::Foundation::NTSTATUS);
}

/// The real effects.
pub(super) struct OsWatch {
    clock: SystemClock,
    checker: OnceLock<Option<Checker>>,
}

impl OsWatch {
    pub(super) fn new() -> OsWatch {
        OsWatch {
            clock: SystemClock::new(),
            checker: OnceLock::new(),
        }
    }
}

impl Watch for OsWatch {
    type Pipe = WatchPipe;

    fn request(&self) -> Result<(), StartError> {
        request_watch().map_err(|error| match error {
            TaskError::NotRegistered => StartError::NotRegistered,
            other @ (TaskError::Com { .. }
            | TaskError::Path(_)
            | TaskError::Folder(_)
            | TaskError::Session(_)
            | TaskError::WatchKey(_)) => StartError::Failed(other.to_string()),
        })
    }

    fn pipe_name(&self) -> Result<Option<String>, String> {
        published_name()
    }

    fn connect(&self, name: &str) -> Result<WatchPipe, ConnectError> {
        let pipe = open(name)?;
        let facts = server_facts(&pipe.handle);
        let admin = admin_os::program_files()
            .map(|folder| admin_copy::admin_exe(&folder))
            .map_err(|error| ConnectError::Failed(format!("the admin copy's path: {error}")))?;
        let device = drive_device(&admin).map_err(ConnectError::Failed)?;
        let unread = check_server(&facts, &admin, &device).map_err(ConnectError::NotTheServer)?;
        if !unread.is_empty() {
            tracing::info!(
                ?unread,
                "the denial reporter's pipe is LOCAL SYSTEM's; taken on its owner, these unread"
            );
        }
        Ok(pipe)
    }

    fn turned_off(&self) -> bool {
        /// A value that isn't a readable `DWORD` (a `REG_SZ "0"`, say) was logged as off.
        static LOGGED: AtomicBool = AtomicBool::new(false);
        let key = HSTRING::from(POLICY_KEY);
        let value = HSTRING::from(POLICY_VALUE);
        let read = read_dword(PCWSTR(key.as_ptr()), PCWSTR(value.as_ptr()));
        if let Err(error) = &read
            && !LOGGED.swap(true, Ordering::Relaxed)
        {
            tracing::warn!(
                %error,
                "the denial reporter's policy DenialReporter isn't a readable DWORD: treated as off"
            );
        }
        policy_says_off(&read)
    }

    fn last_exit(&self) -> Option<u32> {
        last_watch_result().ok().map(i32::cast_unsigned)
    }

    fn would_deny(&self, pool_user: &Sid, path: &Path, access: Access) -> Option<bool> {
        self.checker
            .get_or_init(Checker::new)
            .as_ref()?
            .would_deny(pool_user, path, access)
    }

    fn now(&self) -> Duration {
        self.clock.now()
    }

    fn sleep(&self, wait: Duration) {
        let ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: it only waits.
        unsafe { Sleep(ms) };
    }

    fn log(&self, error: bool, line: &str) {
        if error {
            tracing::warn!("{line}");
        } else {
            tracing::info!("{line}");
        }
    }
}

/// Whether `path`'s descriptor refuses `access` to the pool user `pool_user`, worked out as
/// this user with `AuthzAccessCheck` (`acl/authz.rs`): `None` when it can't tell. For PR6's
/// `Launchers::would_deny` and the host-side source's confirmation.
pub fn would_deny(pool_user: &Sid, path: &Path, access: Access) -> Option<bool> {
    authz::would_deny(pool_user, path, access)
}

/// The owner of the object `handle` is open on, as `kind`, needing `READ_CONTROL`.
fn owner_of(handle: HANDLE, kind: SE_OBJECT_TYPE) -> Result<Sid, String> {
    let mut owner = PSID::default();
    let mut descriptor = PSECURITY_DESCRIPTOR::default();
    // SAFETY: a live handle opened with READ_CONTROL; both out-pointers are live; `owner`
    // points into the descriptor, freed below once the owner is read.
    let status = unsafe {
        GetSecurityInfo(
            handle,
            kind,
            OWNER_SECURITY_INFORMATION,
            Some(&raw mut owner),
            None,
            None,
            None,
            Some(&raw mut descriptor),
        )
    };
    let read = if status.is_ok() {
        sid_text(owner).map_err(|error| error.to_string())
    } else {
        Err(format!("GetSecurityInfo failed (error {})", status.0))
    };
    if !descriptor.0.is_null() {
        // SAFETY: allocated by GetSecurityInfo with LocalAlloc, freed once, after the read.
        unsafe { LocalFree(Some(HLOCAL(descriptor.0))) };
    }
    read
}

/// The published name, read on a handle to the key whose owner was checked first: `Ok(None)`
/// when the key or value isn't there, `Err` when LOCAL SYSTEM doesn't own the key.
fn published_name() -> Result<Option<String>, String> {
    let mut key = HKEY::default();
    // SAFETY: a live HSTRING; `key` is a live out-pointer, closed below.
    let opened = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &HSTRING::from(WATCH_KEY),
            None,
            KEY_READ | KEY_WOW64_64KEY,
            &raw mut key,
        )
    };
    if opened.is_err() {
        return Ok(None);
    }
    let read = owner_of(HANDLE(key.0), SE_REGISTRY_KEY).and_then(|owner| {
        if owner.as_str() == SYSTEM {
            Ok(value_of(key))
        } else {
            Err("the key publishing its name isn't owned by LOCAL SYSTEM".to_owned())
        }
    });
    // SAFETY: opened above, closed once.
    let _closed = unsafe { RegCloseKey(key) };
    read
}

/// The `REG_SZ` [`PIPE_VALUE`] of the open `key`, up to its first NUL.
fn value_of(key: HKEY) -> Option<String> {
    let mut buffer = vec![0u16; 512];
    let mut size = u32::try_from(buffer.len() * 2).ok()?;
    // SAFETY: a live key and HSTRING; `buffer` is writable for `size` bytes.
    let read = unsafe {
        RegGetValueW(
            key,
            None,
            &HSTRING::from(PIPE_VALUE),
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr().cast()),
            Some(&raw mut size),
        )
    };
    if read.is_err() {
        return None;
    }
    buffer.truncate(usize::try_from(size / 2).ok()?);
    let text = buffer.split(|unit| *unit == 0).next().unwrap_or_default();
    String::from_utf16(text).ok()
}

/// Opens the pipe `name`: `GENERIC_READ` alone, overlapped, identification-only quality of
/// service, so whoever serves it may learn who this is but never act as them.
fn open(name: &str) -> Result<WatchPipe, ConnectError> {
    // SAFETY: a live HSTRING; no security attributes; the handle is owned by `Handle`.
    let opened = unsafe {
        CreateFileW(
            &HSTRING::from(name),
            GENERIC_READ.0,
            FILE_SHARE_NONE,
            None,
            OPEN_EXISTING,
            FILE_FLAGS_AND_ATTRIBUTES(
                FILE_FLAG_OVERLAPPED.0 | SECURITY_SQOS_PRESENT.0 | SECURITY_IDENTIFICATION.0,
            ),
            None,
        )
    };
    match opened {
        Ok(handle) => Ok(WatchPipe {
            handle: Handle(handle),
        }),
        Err(error)
            if [ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY]
                .iter()
                .any(|code| error.code() == code.to_hresult()) =>
        {
            Err(ConnectError::Absent)
        }
        Err(error) if error.code() == ERROR_ACCESS_DENIED.to_hresult() => Err(
            ConnectError::NotTheServer("the pipe refuses this user".to_owned()),
        ),
        Err(error) => Err(ConnectError::Failed(os_error(&error).to_string())),
    }
}

/// Who serves `pipe`: its owner, its session, and its process's user and image, read with no
/// handle to the process. A fact Windows won't give is `Err`.
fn server_facts(pipe: &Handle) -> ServerFacts {
    let owner = owner_of(pipe.0, SE_KERNEL_OBJECT);
    let mut session = 0u32;
    // SAFETY: a live client end; `session` is a live out-pointer.
    let session = unsafe { GetNamedPipeServerSessionId(pipe.0, &raw mut session) }
        .map(|()| session)
        .map_err(|error| os_error(&error).to_string());
    let mut pid = 0u32;
    // SAFETY: a live client end; `pid` is a live out-pointer.
    if let Err(error) = unsafe { GetNamedPipeServerProcessId(pipe.0, &raw mut pid) } {
        let why = format!("its process: {}", os_error(&error));
        return ServerFacts {
            owner,
            session,
            user: Err(why.clone()),
            image: Err(why),
        };
    }
    let user = listed_users().and_then(|mut listed| match listed.remove(&pid) {
        Some((Some(user), _session)) => Ok(user),
        Some((None, _session)) => Err("the process list gives no user".to_owned()),
        None => Err("the process is no longer listed".to_owned()),
    });
    ServerFacts {
        owner,
        session,
        user,
        image: image_of(pid),
    }
}

/// `SYSTEM_PROCESS_ID_INFORMATION`: a process id in, its image name out.
#[repr(C)]
struct ProcessIdInformation {
    process_id: HANDLE,
    image_name: UNICODE_STRING,
}

/// The image of process `pid` as the kernel names it (`\Device\HarddiskVolumeN\...`), read by
/// its id alone; `Err` when the kernel won't give it to this user, or no process has the id.
fn image_of(pid: u32) -> Result<String, String> {
    let mut name = vec![0u16; usize::from(u16::MAX / 2)];
    let mut info = ProcessIdInformation {
        process_id: HANDLE(usize::try_from(pid).map_err(|error| error.to_string())? as *mut _),
        image_name: UNICODE_STRING {
            Length: 0,
            MaximumLength: u16::try_from(name.len() * 2).map_err(|error| error.to_string())?,
            Buffer: PWSTR(name.as_mut_ptr()),
        },
    };
    let size =
        u32::try_from(size_of::<ProcessIdInformation>()).map_err(|error| error.to_string())?;
    // SAFETY: `info` is live and `size` bytes long, and its buffer is `name`, live and
    // `MaximumLength` bytes long, for the call.
    let status = unsafe {
        nt::NtQuerySystemInformation(
            PROCESS_ID_INFORMATION,
            (&raw mut info).cast(),
            size,
            std::ptr::null_mut(),
        )
    };
    if status == STATUS_ACCESS_DENIED {
        return Err("the kernel won't give it".to_owned());
    }
    if status.is_err() {
        return Err(format!("NTSTATUS {:#010x}", status.0));
    }
    let units = usize::from(info.image_name.Length) / 2;
    let image = name.get(..units).ok_or("its length")?;
    String::from_utf16(image).map_err(|error| error.to_string())
}

/// The device `admin_exe`'s drive names (`QueryDosDeviceW`).
fn drive_device(admin_exe: &Path) -> Result<String, String> {
    let text = admin_exe.to_string_lossy();
    let drive = text.get(..2).ok_or("the admin copy's path has no drive")?;
    let mut device = vec![0u16; 1024];
    // SAFETY: a live HSTRING; `device` is writable for its length.
    let written = unsafe { QueryDosDeviceW(&HSTRING::from(drive), Some(&mut device)) };
    if written == 0 {
        return Err(format!(
            "the drive {drive}'s device: {}",
            io::Error::last_os_error()
        ));
    }
    let end = device
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(device.len());
    String::from_utf16(device.get(..end).unwrap_or_default()).map_err(|error| error.to_string())
}

/// The pipe's client end, read with a limit on every wait.
pub(super) struct WatchPipe {
    handle: Handle,
}

// SAFETY: the handle is a kernel handle, used by one thread at a time (`&mut self`).
unsafe impl Send for WatchPipe {}

impl Pipe for WatchPipe {
    fn read(&mut self, buf: &mut [u8], wait: Duration) -> io::Result<Option<usize>> {
        let mut io = Overlapped::new()?;
        let len = buf.len().min(64 * 1024);
        // SAFETY: `buf` and `io` outlive the operation, which ends (read, or cancelled and
        // waited for) before either drops.
        let started = unsafe {
            ReadFile(
                self.handle.0,
                buf.get_mut(..len),
                None,
                Some(io.as_mut_ptr()),
            )
        };
        match started {
            Ok(()) => {}
            Err(error) if error.code() == ERROR_IO_PENDING.to_hresult() => {
                let ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
                // SAFETY: the operation's live event.
                let waited = unsafe { WaitForSingleObject(io.event(), ms) };
                if waited == WAIT_TIMEOUT {
                    // SAFETY: the operation was started with this OVERLAPPED on this handle.
                    drop(unsafe { CancelIoEx(self.handle.0, Some(io.as_mut_ptr())) });
                } else if waited != WAIT_OBJECT_0 {
                    // Make sure nothing writes into `buf` after this returns.
                    // SAFETY: as above.
                    drop(unsafe { CancelIoEx(self.handle.0, Some(io.as_mut_ptr())) });
                    drop(io.result(&self.handle));
                    return Err(io::Error::last_os_error());
                }
            }
            Err(error) => {
                let error = os_error(&error);
                return if error.raw_os_error().is_some_and(ended_code) {
                    Ok(Some(0))
                } else {
                    Err(error)
                };
            }
        }
        match io.result(&self.handle) {
            Ok(0) => Ok(None),
            Ok(read) => Ok(Some(read)),
            Err(error) if error.raw_os_error().is_some_and(ended_code) => Ok(Some(0)),
            Err(error)
                if error
                    .raw_os_error()
                    .is_some_and(|code| code.cast_unsigned() == ERROR_OPERATION_ABORTED.0) =>
            {
                Ok(None)
            }
            Err(error) => Err(error),
        }
    }
}
