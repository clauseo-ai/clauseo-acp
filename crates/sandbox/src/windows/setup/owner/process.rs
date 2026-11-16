//! A process handle for [`Owner`](super::Owner)'s checks: its creation time, its recorded
//! parent and its image, each read through `PROCESS_QUERY_LIMITED_INFORMATION`. Split from
//! `token.rs` for its size.
#![expect(unsafe_code, reason = "FFI to the process calls")]

use std::ffi::c_void;

use windows::Wdk::System::Threading::{NtQueryInformationProcess, ProcessBasicInformation};
use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};

use super::MachineError;

/// A Win32 failure as its call and code.
fn os(call: &'static str, error: &windows::core::Error) -> MachineError {
    MachineError::Os {
        call,
        code: error.code().0.cast_unsigned() & 0xFFFF,
    }
}

/// `PROCESS_BASIC_INFORMATION`, declared here so the `windows` crate's `Win32_System_Kernel`
/// (for its `PEB` pointer) isn't needed; the same `repr(C)` layout.
#[repr(C)]
#[derive(Default)]
struct BasicInformation {
    exit_status: i32,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
}

/// The process ID of the parent Windows recorded for the process behind `handle`; 0 when none.
fn recorded_parent_of(handle: HANDLE) -> Result<u32, MachineError> {
    let mut info = BasicInformation::default();
    let size = u32::try_from(size_of::<BasicInformation>()).unwrap_or(u32::MAX);
    let mut written = 0u32;
    // SAFETY: `handle` is open (or the current process's pseudo-handle) with at least
    // PROCESS_QUERY_LIMITED_INFORMATION; `info` is a live, writable struct of `size` bytes with
    // PROCESS_BASIC_INFORMATION's layout; `written` is a live out-pointer.
    let status = unsafe {
        NtQueryInformationProcess(
            handle,
            ProcessBasicInformation,
            (&raw mut info).cast::<c_void>(),
            size,
            &raw mut written,
        )
    };
    if status.is_err() {
        return Err(MachineError::Os {
            call: "NtQueryInformationProcess",
            code: status.0.cast_unsigned(),
        });
    }
    u32::try_from(info.inherited_from_unique_process_id)
        .map_err(|error| MachineError::Other(format!("the recorded parent's process ID: {error}")))
}

/// A process handle; closed on drop unless it is the current process's pseudo-handle.
pub(crate) struct Process {
    pub(super) handle: HANDLE,
    owned: bool,
}

impl Process {
    /// Opens process `pid` for `PROCESS_QUERY_LIMITED_INFORMATION`: enough for its times and to
    /// open its token. Holding the handle keeps the process object, so its ID can't be reused
    /// while it is open.
    pub(super) fn open(pid: u32) -> Result<Process, MachineError> {
        // SAFETY: no pointers; the handle is owned by `Process` from here.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
            .map_err(|error| os("OpenProcess", &error))?;
        Ok(Process {
            handle,
            owned: true,
        })
    }

    /// This process, through its pseudo-handle.
    pub(super) fn current() -> Process {
        Process {
            // SAFETY: no arguments; the pseudo-handle needs no close.
            handle: unsafe { GetCurrentProcess() },
            owned: false,
        }
    }

    /// The process ID of the parent Windows recorded for it; 0 when none.
    pub(super) fn recorded_parent(&self) -> Result<u32, MachineError> {
        recorded_parent_of(self.handle)
    }

    /// Its image's full Win32 path (`QueryFullProcessImageNameW`).
    pub(super) fn image(&self) -> Result<std::path::PathBuf, MachineError> {
        use std::os::windows::ffi::OsStringExt as _;
        let mut buffer = vec![0u16; 1024];
        let mut size = u32::try_from(buffer.len()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open with PROCESS_QUERY_LIMITED_INFORMATION; `buffer` is
        // writable for `size` units, and `size` is a live in-out pointer.
        unsafe {
            QueryFullProcessImageNameW(
                self.handle,
                PROCESS_NAME_WIN32,
                windows::core::PWSTR(buffer.as_mut_ptr()),
                &raw mut size,
            )
        }
        .map_err(|error| os("QueryFullProcessImageNameW", &error))?;
        buffer.truncate(usize::try_from(size).unwrap_or(0));
        Ok(std::ffi::OsString::from_wide(&buffer).into())
    }

    /// Its creation time, in `FILETIME` units.
    pub(super) fn created(&self) -> Result<u64, MachineError> {
        let mut times = [FILETIME::default(); 4];
        let [created, exited, kernel, user] = &mut times;
        // SAFETY: the handle is open; the four out-pointers are live and distinct.
        unsafe {
            GetProcessTimes(
                self.handle,
                &raw mut *created,
                &raw mut *exited,
                &raw mut *kernel,
                &raw mut *user,
            )
        }
        .map_err(|error| os("GetProcessTimes", &error))?;
        Ok(u64::from(created.dwHighDateTime) << 32 | u64::from(created.dwLowDateTime))
    }
}

impl Drop for Process {
    fn drop(&mut self) {
        if self.owned {
            // SAFETY: the handle came from OpenProcess and is closed once, here. A failed close
            // leaves nothing to do.
            drop(unsafe { CloseHandle(self.handle) });
        }
    }
}
