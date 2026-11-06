//! Windows: a handle to the guard's parent process, which a thread waits on.
//!
//! Windows keeps no parent link a process can wait on, so the parent's pid comes from a
//! Toolhelp process snapshot. A pid can be reused once its process is gone, so the process
//! opened under it counts as the parent only if it started before the guard did. The handle is
//! opened first thing in the guard's run, so a parent that exits later still signals it.
#![expect(
    unsafe_code,
    reason = "FFI to Toolhelp and the process APIs, to wait for the guard's parent"
)]

use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};

use windows::Win32::Foundation::{
    CloseHandle, ERROR_INVALID_PARAMETER, FILETIME, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    PROCESS_SYNCHRONIZE, WaitForSingleObject,
};

/// The guard's parent process, open for waiting.
pub(super) struct Parent {
    handle: OwnedHandle,
}

impl Parent {
    /// Blocks until the parent exits; whether it did (false if the wait itself failed).
    pub(super) fn wait(&self) -> bool {
        self.wait_for(INFINITE)
    }

    /// Whether the parent has exited already; does not block.
    pub(super) fn exited(&self) -> bool {
        self.wait_for(0)
    }

    fn wait_for(&self, millis: u32) -> bool {
        // SAFETY: the handle is the open process handle `self` owns, with SYNCHRONIZE.
        let ended = unsafe { WaitForSingleObject(HANDLE(self.handle.as_raw_handle()), millis) };
        ended == WAIT_OBJECT_0
    }
}

/// What [`parent`] found.
pub(super) enum Lookup {
    /// The parent, open for waiting.
    Found(Parent),
    /// The parent has exited already: nothing runs under its pid, or what does started after
    /// the guard (the pid was reused).
    Gone,
    /// The parent could not be told (no snapshot entry, or Windows refused the handle).
    Unknown,
}

/// The guard's parent: open, gone, or not to be told.
pub(super) fn parent() -> Lookup {
    let Some(pid) = parent_pid(std::process::id()) else {
        return Lookup::Unknown;
    };
    // SAFETY: opening a process by id takes no pointer; the handle is owned below.
    let opened = unsafe {
        OpenProcess(
            PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION,
            false,
            pid,
        )
    };
    let raw = match opened {
        Ok(raw) => raw,
        // OpenProcess's answer for a pid nothing runs under.
        Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() => return Lookup::Gone,
        Err(_) => return Lookup::Unknown,
    };
    // SAFETY: `raw` is the open handle OpenProcess just returned, which nothing else owns; the
    // `OwnedHandle` closes it once, on drop.
    let handle = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    // SAFETY: GetCurrentProcess returns a pseudo handle that needs no closing.
    let current = unsafe { GetCurrentProcess() };
    let (Some(own), Some(parents)) = (started(current), started(HANDLE(handle.as_raw_handle())))
    else {
        return Lookup::Unknown;
    };
    if parents <= own {
        Lookup::Found(Parent { handle })
    } else {
        Lookup::Gone
    }
}

/// The creation time of process `handle`, in 100 ns units.
fn started(handle: HANDLE) -> Option<u64> {
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    // SAFETY: `handle` is a live process handle with query rights, and the four pointers are
    // to live FILETIMEs.
    unsafe {
        GetProcessTimes(
            handle,
            &raw mut created,
            &raw mut exited,
            &raw mut kernel,
            &raw mut user,
        )
    }
    .ok()?;
    Some((u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime))
}

/// The parent pid of process `pid`, from a process snapshot.
fn parent_pid(pid: u32) -> Option<u32> {
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).ok()?,
        ..PROCESSENTRY32W::default()
    };
    // SAFETY: a process snapshot takes no pointer; its handle is closed below, and nothing
    // between here and there returns early.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }.ok()?;
    let mut found = None;
    // SAFETY: `snapshot` is the live snapshot handle and `entry` a PROCESSENTRY32W whose
    // `dwSize` is set, as Process32FirstW and Process32NextW require.
    let mut next = unsafe { Process32FirstW(snapshot, &raw mut entry) };
    while next.is_ok() {
        if entry.th32ProcessID == pid {
            found = Some(entry.th32ParentProcessID);
            break;
        }
        // SAFETY: as for Process32FirstW above.
        next = unsafe { Process32NextW(snapshot, &raw mut entry) };
    }
    // SAFETY: `snapshot` is a handle this function owns and closes only here.
    drop(unsafe { CloseHandle(snapshot) });
    found
}
