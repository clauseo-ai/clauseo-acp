//! The Win32 calls of the security probes (`probe_sec.rs`) on processes and handles: the parent
//! pid, opens of a runner's process and token, a process's image name, this process's own handle
//! table with each pipe handle's name and end, and `IsProcessInJob`. Each gives a plain value or
//! the Win32 code (an NTSTATUS in hex for the `Nt` calls), never a panic.
#![expect(
    unsafe_code,
    reason = "FFI to the process, token, handle-table, object-name and pipe calls"
)]

use std::ffi::c_void;
use std::sync::mpsc;
use std::time::Duration;

use windows::Wdk::Foundation::{NtQueryObject, OBJECT_INFORMATION_CLASS, OBJECT_NAME_INFORMATION};
use windows::Wdk::System::Threading::{
    NtQueryInformationProcess, PROCESSINFOCLASS, ProcessBasicInformation,
};
use windows::Win32::Foundation::{HANDLE, NTSTATUS};
use windows::Win32::Security::TOKEN_ACCESS_MASK;
use windows::Win32::Storage::FileSystem::GetFileType;
use windows::Win32::System::JobObjects::IsProcessInJob;
use windows::Win32::System::Pipes::{GetNamedPipeInfo, NAMED_PIPE_MODE, PIPE_SERVER_END};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessHandleCount, OpenProcess, OpenProcessToken, PROCESS_ACCESS_RIGHTS,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::{BOOL, PWSTR};

use super::win::{Owned, code_of};

/// `ProcessHandleInformation`: a process's handle table (Windows 8 and later), which needs only
/// `PROCESS_QUERY_INFORMATION` on the process; this process's own pseudo-handle has it.
const PROCESS_HANDLE_INFORMATION: PROCESSINFOCLASS = PROCESSINFOCLASS(51);
/// `ObjectNameInformation`.
const OBJECT_NAME: OBJECT_INFORMATION_CLASS = OBJECT_INFORMATION_CLASS(1);
/// `STATUS_INFO_LENGTH_MISMATCH`.
const LENGTH_MISMATCH: i32 = i32::from_ne_bytes(0xC000_0004_u32.to_ne_bytes());
/// `OBJ_INHERIT` in a handle's attributes.
pub(crate) const OBJ_INHERIT: u32 = 0x2;

/// `PROCESS_BASIC_INFORMATION`, laid out here: the crate's needs `Win32_System_Kernel`.
#[repr(C)]
#[derive(Default)]
struct BasicInformation {
    exit_status: i32,
    peb: usize,
    affinity: usize,
    base_priority: i32,
    pid: usize,
    parent: usize,
}

/// `PROCESS_HANDLE_SNAPSHOT_INFORMATION`'s head; its entries follow.
#[repr(C)]
struct SnapshotHead {
    count: usize,
    reserved: usize,
}

/// `PROCESS_HANDLE_TABLE_ENTRY_INFO`.
#[repr(C)]
#[derive(Clone, Copy)]
struct TableEntry {
    handle: usize,
    handle_count: usize,
    pointer_count: usize,
    granted: u32,
    type_index: u32,
    attributes: u32,
    reserved: u32,
}

/// One handle of this process's table.
#[derive(Debug, Clone, Copy)]
pub(crate) struct HandleEntry {
    /// The handle's value.
    pub(crate) value: usize,
    /// The object type's index (the same for every handle of one type).
    pub(crate) type_index: u32,
    /// `OBJ_INHERIT` and the rest.
    pub(crate) attributes: u32,
    /// The access the handle grants.
    pub(crate) granted: u32,
}

fn nt(status: NTSTATUS) -> String {
    format!("NTSTATUS {:#x}", status.0.cast_unsigned())
}

/// The handle `value` as a `HANDLE`: a table index, not an address.
fn handle(value: usize) -> HANDLE {
    HANDLE(std::ptr::without_provenance_mut::<c_void>(value))
}

/// The pid of the process that started this one (`InheritedFromUniqueProcessId`).
pub(crate) fn parent_pid() -> Result<u32, String> {
    let mut info = BasicInformation::default();
    let len = u32::try_from(size_of::<BasicInformation>()).map_err(|e| e.to_string())?;
    let mut got = 0u32;
    // SAFETY: the pseudo-handle needs no closing; `info` is a live, writable struct of `len`
    // bytes laid out as the class's structure.
    let status = unsafe {
        NtQueryInformationProcess(
            GetCurrentProcess(),
            ProcessBasicInformation,
            (&raw mut info).cast(),
            len,
            &raw mut got,
        )
    };
    if status.0 < 0 {
        return Err(nt(status));
    }
    u32::try_from(info.parent).map_err(|e| e.to_string())
}

/// The code `OpenProcess(access)` gives for `pid`; 0 when it opened (closed again at once).
pub(crate) fn open_code(pid: u32, access: u32) -> u32 {
    // SAFETY: plain call; a handle that comes back is closed by `Owned`.
    match unsafe { OpenProcess(PROCESS_ACCESS_RIGHTS(access), false, pid) } {
        Ok(opened) => {
            drop(Owned(opened));
            0
        }
        Err(error) => code_of(&error),
    }
}

/// `OpenProcessToken(access)` on `pid`, through a `PROCESS_QUERY_LIMITED_INFORMATION` handle:
/// `Err(code)` when that open fails, else `Ok` with the token open's code (0 when it opened).
pub(crate) fn token_code(pid: u32, access: u32) -> Result<u32, u32> {
    // SAFETY: plain call; the handle is closed by `Owned`.
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(opened) => Owned(opened),
        Err(error) => return Err(code_of(&error)),
    };
    let mut token = HANDLE::default();
    // SAFETY: the process handle is live; `token` is a live out-pointer, closed by `Owned`.
    match unsafe { OpenProcessToken(process.0, TOKEN_ACCESS_MASK(access), &raw mut token) } {
        Ok(()) => {
            drop(Owned(token));
            Ok(0)
        }
        Err(error) => Ok(code_of(&error)),
    }
}

/// The image path of `pid`, through a `PROCESS_QUERY_LIMITED_INFORMATION` handle; the code
/// when that fails.
pub(crate) fn image_of(pid: u32) -> Result<String, u32> {
    // SAFETY: plain call; the handle is closed by `Owned`.
    let process = match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
        Ok(opened) => Owned(opened),
        Err(error) => return Err(code_of(&error)),
    };
    let mut name = vec![0u16; 1024];
    let mut len = u32::try_from(name.len()).unwrap_or(0);
    // SAFETY: the handle is live; `name` is writable for `len` characters.
    unsafe {
        QueryFullProcessImageNameW(
            process.0,
            PROCESS_NAME_WIN32,
            PWSTR(name.as_mut_ptr()),
            &raw mut len,
        )
    }
    .map_err(|e| code_of(&e))?;
    let end = usize::try_from(len).unwrap_or(0);
    Ok(String::from_utf16_lossy(
        name.get(..end).unwrap_or_default(),
    ))
}

/// `GetProcessHandleCount` of this process; the code when it fails.
pub(crate) fn handle_count() -> Result<u32, u32> {
    let mut count = 0u32;
    // SAFETY: the pseudo-handle needs no closing; `count` is a live out-pointer.
    unsafe { GetProcessHandleCount(GetCurrentProcess(), &raw mut count) }
        .map_err(|e| code_of(&e))?;
    Ok(count)
}

/// This process's handle table (`NtQueryInformationProcess(ProcessHandleInformation)`).
pub(crate) fn own_handles() -> Result<Vec<HandleEntry>, String> {
    let mut words = 8192usize;
    for _ in 0..8 {
        let mut buffer = vec![0u64; words];
        let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(|e| e.to_string())?;
        let mut needed = 0u32;
        // SAFETY: the pseudo-handle needs no closing; `buffer` is live, writable, `len` bytes
        // and aligned for the snapshot.
        let status = unsafe {
            NtQueryInformationProcess(
                GetCurrentProcess(),
                PROCESS_HANDLE_INFORMATION,
                buffer.as_mut_ptr().cast(),
                len,
                &raw mut needed,
            )
        };
        if status.0 == LENGTH_MISMATCH {
            let bytes = usize::try_from(needed).map_err(|e| e.to_string())?;
            words = words.max(bytes.div_ceil(8)) * 2;
            continue;
        }
        if status.0 < 0 {
            return Err(nt(status));
        }
        return Ok(entries(&buffer));
    }
    Err("the handle table kept growing".to_owned())
}

/// The entries of the snapshot in `buffer`, as many as fit.
fn entries(buffer: &[u64]) -> Vec<HandleEntry> {
    // SAFETY: the call wrote a snapshot head at the start of the aligned buffer.
    let head = unsafe { &*buffer.as_ptr().cast::<SnapshotHead>() };
    let room =
        size_of_val(buffer).saturating_sub(size_of::<SnapshotHead>()) / size_of::<TableEntry>();
    let count = head.count.min(room);
    // SAFETY: `count` entries follow the head inside `buffer`, aligned as usize is.
    let table = unsafe {
        std::slice::from_raw_parts(
            buffer
                .as_ptr()
                .cast::<SnapshotHead>()
                .add(1)
                .cast::<TableEntry>(),
            count,
        )
    };
    table
        .iter()
        .map(|entry| HandleEntry {
            value: entry.handle,
            type_index: entry.type_index,
            attributes: entry.attributes,
            granted: entry.granted,
        })
        .collect()
}

/// `GetFileType` of this process's handle `value` (`FILE_TYPE_UNKNOWN`, 0, for one that isn't a
/// file).
pub(crate) fn file_type(value: usize) -> u32 {
    // SAFETY: the value comes from this process's own handle table; a handle that isn't a file
    // gives FILE_TYPE_UNKNOWN.
    unsafe { GetFileType(handle(value)) }.0
}

/// Whether the pipe handle `value` is the server's end (`GetNamedPipeInfo`); the code when the
/// call fails.
pub(crate) fn pipe_server_end(value: usize) -> Result<bool, u32> {
    let mut flags = NAMED_PIPE_MODE::default();
    // SAFETY: the value is one of this process's pipe handles; `flags` is a live out-pointer.
    unsafe { GetNamedPipeInfo(handle(value), Some(&raw mut flags), None, None, None) }
        .map_err(|e| code_of(&e))?;
    Ok(flags.0 & PIPE_SERVER_END.0 != 0)
}

/// The object name of this process's handle `value` (`NtQueryObject(ObjectNameInformation)`),
/// asked on a thread of its own: the call can block on a synchronous pipe with I/O pending, so
/// one past `timeout` is `Err("hung")`, and that thread is left to the process's exit.
pub(crate) fn object_name(value: usize, timeout: Duration) -> Result<String, String> {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || drop(send.send(name_now(value))));
    receive
        .recv_timeout(timeout)
        .unwrap_or_else(|_late| Err("hung".to_owned()))
}

fn name_now(value: usize) -> Result<String, String> {
    let mut buffer = vec![0u64; 1024];
    let len = u32::try_from(size_of_val(buffer.as_slice())).map_err(|e| e.to_string())?;
    // SAFETY: the value is one of this process's handles; `buffer` is live, writable, `len`
    // bytes and aligned for OBJECT_NAME_INFORMATION.
    let status = unsafe {
        NtQueryObject(
            Some(handle(value)),
            OBJECT_NAME,
            Some(buffer.as_mut_ptr().cast()),
            len,
            None,
        )
    };
    if status.0 < 0 {
        return Err(nt(status));
    }
    // SAFETY: the call wrote an OBJECT_NAME_INFORMATION at the start of `buffer`; its string
    // points into `buffer` (or is empty).
    let info = unsafe { &*buffer.as_ptr().cast::<OBJECT_NAME_INFORMATION>() };
    let units = usize::from(info.Name.Length) / 2;
    if units == 0 || info.Name.Buffer.is_null() {
        return Ok(String::new());
    }
    // SAFETY: `Length` bytes of UTF-16 at `Buffer`, inside `buffer`, which is still live.
    let name = unsafe { std::slice::from_raw_parts(info.Name.Buffer.0, units) };
    Ok(String::from_utf16_lossy(name))
}

/// `IsProcessInJob(process, NULL)`: whether `process` is in any job; the code when it fails.
pub(crate) fn in_any_job(process: HANDLE) -> Result<bool, u32> {
    let mut inside = BOOL::default();
    // SAFETY: the caller's process handle is live; `inside` is a live out-pointer.
    unsafe { IsProcessInJob(process, None, &raw mut inside) }.map_err(|e| code_of(&e))?;
    Ok(inside.as_bool())
}
