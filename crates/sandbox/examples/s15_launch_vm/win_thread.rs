//! The Toolhelp calls of the `runners` probe: the threads of a pid (`TH32CS_SNAPTHREAD`), the
//! children of a set of pids by image name (`TH32CS_SNAPPROCESS`), and `OpenThread`. Toolhelp is
//! declared here against kernel32 directly: the crate's `windows` features don't hold
//! `Win32_System_Diagnostics_ToolHelp`, and an example shouldn't widen the product's list.
#![expect(
    unsafe_code,
    reason = "FFI to the Toolhelp snapshot and OpenThread calls"
)]

use std::ffi::c_void;

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Threading::{OpenThread, THREAD_ACCESS_RIGHTS};

use super::win::{Owned, code_of};

/// `TH32CS_SNAPPROCESS`.
const SNAP_PROCESS: u32 = 0x2;
/// `TH32CS_SNAPTHREAD`.
const SNAP_THREAD: u32 = 0x4;
/// `ERROR_NO_MORE_FILES`: the snapshot's walk ended.
const NO_MORE_FILES: u32 = 18;

/// `THREADENTRY32`.
#[repr(C)]
#[derive(Default)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    thread: u32,
    owner: u32,
    base_priority: i32,
    delta_priority: i32,
    flags: u32,
}

/// `PROCESSENTRY32W`.
#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    pid: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    exe: [u16; 260],
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
    fn Thread32First(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: *mut c_void, entry: *mut ThreadEntry) -> i32;
    fn Process32FirstW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: *mut c_void, entry: *mut ProcessEntry) -> i32;
    fn GetLastError() -> u32;
}

/// A Toolhelp snapshot of `flags`, closed on drop; the code when it can't be taken.
fn snapshot(flags: u32) -> Result<Owned, u32> {
    // SAFETY: plain call; a snapshot that comes back is closed by `Owned`.
    let snap = unsafe { CreateToolhelp32Snapshot(flags, 0) };
    if snap.addr() == usize::MAX || snap.is_null() {
        // SAFETY: plain call.
        return Err(unsafe { GetLastError() });
    }
    Ok(Owned(HANDLE(snap)))
}

/// The end of a walk: `Ok` at `ERROR_NO_MORE_FILES`, else the code.
fn walk_end() -> Result<(), u32> {
    // SAFETY: plain call.
    match unsafe { GetLastError() } {
        NO_MORE_FILES => Ok(()),
        code => Err(code),
    }
}

/// The ids of the threads `pid` owns, from a system-wide thread snapshot (nothing is opened).
pub(crate) fn threads_of(pid: u32) -> Result<Vec<u32>, u32> {
    let snap = snapshot(SNAP_THREAD)?;
    let mut entry = ThreadEntry {
        size: u32::try_from(size_of::<ThreadEntry>()).unwrap_or(0),
        ..ThreadEntry::default()
    };
    let mut found = Vec::new();
    // SAFETY: the snapshot is live; `entry` is a live struct whose `size` is set.
    let mut more = unsafe { Thread32First(snap.0.0, &raw mut entry) } != 0;
    while more {
        if entry.owner == pid {
            found.push(entry.thread);
        }
        // SAFETY: as above.
        more = unsafe { Thread32Next(snap.0.0, &raw mut entry) } != 0;
    }
    walk_end()?;
    Ok(found)
}

/// The processes whose parent pid is one of `parents` and whose image is `image` (no case):
/// each `(parent, pid)`.
pub(crate) fn children_named(parents: &[u32], image: &str) -> Result<Vec<(u32, u32)>, u32> {
    let snap = snapshot(SNAP_PROCESS)?;
    let mut entry = ProcessEntry {
        size: u32::try_from(size_of::<ProcessEntry>()).unwrap_or(0),
        usage: 0,
        pid: 0,
        heap: 0,
        module: 0,
        threads: 0,
        parent: 0,
        priority: 0,
        flags: 0,
        exe: [0; 260],
    };
    let mut found = Vec::new();
    // SAFETY: the snapshot is live; `entry` is a live struct whose `size` is set.
    let mut more = unsafe { Process32FirstW(snap.0.0, &raw mut entry) } != 0;
    while more {
        if parents.contains(&entry.parent) {
            let end = entry
                .exe
                .iter()
                .position(|unit| *unit == 0)
                .unwrap_or(entry.exe.len());
            let name = String::from_utf16_lossy(entry.exe.get(..end).unwrap_or_default());
            if name.eq_ignore_ascii_case(image) {
                found.push((entry.parent, entry.pid));
            }
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snap.0.0, &raw mut entry) } != 0;
    }
    walk_end()?;
    Ok(found)
}

/// The code `OpenThread(access)` gives for `tid`; 0 when it opened (closed again at once).
pub(crate) fn open_thread_code(tid: u32, access: u32) -> u32 {
    // SAFETY: plain call; a handle that comes back is closed by `Owned`.
    match unsafe { OpenThread(THREAD_ACCESS_RIGHTS(access), false, tid) } {
        Ok(opened) => {
            drop(Owned(opened));
            0
        }
        Err(error) => code_of(&error),
    }
}
