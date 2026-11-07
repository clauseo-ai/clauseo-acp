//! The stop's real [`Stopper`]: the record in `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view), the
//! admin copy's path under `FOLDERID_ProgramFiles`, a Toolhelp process snapshot with each
//! process's token user, `TerminateProcess` through the handle that user was read through, the
//! set-up lock, and the Event Log.
#![expect(
    unsafe_code,
    reason = "FFI to the Toolhelp snapshot, OpenProcess and TerminateProcess"
)]

use std::path::PathBuf;

use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, TerminateProcess, WaitForSingleObject,
};

use super::{Failed, Pool, StopExit, Stopper, stop};
use crate::windows::acl::object::Handle;
use crate::windows::admin_copy::{self, os};
use crate::windows::refresh::event_log;
use crate::windows::setup::lock::{Held, SETUP_MUTEX, SetupMutex};
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::owner::this_process;
use crate::windows::setup::processes::{DebugPrivilege, check_pool, user_of};
use crate::windows::setup::record_write::{self, RECORD_KEY};
use crate::windows::setup::strays::Seen;
use crate::windows::sids::Sid;

/// The exit code a stopped process gets.
const STOPPED_EXIT: u32 = 1;
/// How long each stopped process is waited for, in milliseconds.
const EXIT_WAIT_MS: u32 = 3_000;
/// How long the stop waits for the set-up lock before it marks the users, in milliseconds: well
/// inside the task's 2-minute limit.
const LOCK_WAIT_MS: u32 = 30_000;

/// `--stop`: [`stop`] over a [`WindowsStopper`].
pub(crate) fn run() -> StopExit {
    stop(&mut WindowsStopper::new())
}

/// The machine the stop acts on.
pub(crate) struct WindowsStopper {
    /// `SeDebugPrivilege`, enabled for the whole run where the token holds it (SYSTEM's does).
    _debug: Option<DebugPrivilege>,
    /// The last snapshot's processes that could be opened to be terminated, by pid.
    handles: Vec<(u32, Handle)>,
    /// The set-up lock, while held.
    held: Option<Held>,
}

impl WindowsStopper {
    fn new() -> WindowsStopper {
        WindowsStopper {
            _debug: DebugPrivilege::enable(),
            handles: Vec::new(),
            held: None,
        }
    }
}

impl Stopper for WindowsStopper {
    fn is_system(&mut self) -> Result<bool, Failed> {
        this_process()
            .map(|(_elevated, system)| system)
            .map_err(|error| error.to_string())
    }

    fn current_exe(&mut self) -> Result<PathBuf, Failed> {
        os::running_image().map_err(|error| error.to_string())
    }

    fn admin_copy(&mut self) -> Result<PathBuf, Failed> {
        let program_files = os::program_files().map_err(|error| error.to_string())?;
        Ok(admin_copy::admin_exe(&program_files))
    }

    fn pool(&mut self) -> Result<Option<Pool>, Failed> {
        let record = record_write::read(RECORD_KEY).map_err(|error| error.to_string())?;
        Ok(record.map(|record| Pool {
            owner: record.owner,
            users: record.pool,
        }))
    }

    fn check_pool(&mut self, pool: &Pool) -> Result<(), Failed> {
        check_pool(&pool.users, &pool.owner).map_err(|error| error.to_string())
    }

    fn snapshot(&mut self) -> Result<Vec<Seen>, Failed> {
        self.handles.clear();
        let mut seen = Vec::new();
        for pid in snapshot_pids()? {
            let full = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
            // SAFETY: OpenProcess takes plain values; the handle is owned by `Handle` from here.
            let opened = unsafe { OpenProcess(full, false, pid) }.map(|h| (Handle(h), true));
            let opened = opened.or_else(|_no_terminate| {
                // SAFETY: as above, with the query right alone.
                unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
                    .map(|h| (Handle(h), false))
            });
            let Ok((handle, can_stop)) = opened else {
                seen.push(Seen { pid, user: None });
                continue;
            };
            seen.push(Seen {
                pid,
                user: user_of(&handle),
            });
            if can_stop {
                self.handles.push((pid, handle));
            }
        }
        Ok(seen)
    }

    fn own_pid(&mut self) -> u32 {
        // SAFETY: GetCurrentProcessId only reads this process's id.
        unsafe { GetCurrentProcessId() }
    }

    fn terminate(&mut self, pid: u32) -> Result<(), Failed> {
        let Some((_, handle)) = self.handles.iter().find(|(at, _)| *at == pid) else {
            return Err("not opened for termination".to_owned());
        };
        // SAFETY: the handle is open with PROCESS_TERMINATE on the process whose user was read.
        unsafe { TerminateProcess(handle.0, STOPPED_EXIT) }
            .map_err(|error| format!("TerminateProcess failed ({})", error.code().0))?;
        // SAFETY: the handle is open with PROCESS_SYNCHRONIZE; the wait is bounded.
        if unsafe { WaitForSingleObject(handle.0, EXIT_WAIT_MS) } == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err("it hadn't exited when the wait ran out".to_owned())
        }
    }

    fn lock(&mut self) -> Result<bool, Failed> {
        let mutex = SetupMutex::open(SETUP_MUTEX).map_err(|error| error.to_string())?;
        match mutex.acquire(LOCK_WAIT_MS) {
            Ok((held, _acquired)) => {
                self.held = Some(held);
                Ok(true)
            }
            Err(MachineError::LockHeld) => Ok(false),
            Err(error) => Err(error.to_string()),
        }
    }

    fn unlock(&mut self) {
        self.held = None;
    }

    fn mark_for_reset(&mut self, users: &[Sid]) -> Result<(), Failed> {
        record_write::add_reset_sids(RECORD_KEY, users).map_err(|error| error.to_string())
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}

/// Every running process's pid, from a Toolhelp snapshot (`TH32CS_SNAPPROCESS`).
fn snapshot_pids() -> Result<Vec<u32>, Failed> {
    // SAFETY: plain values; the handle is owned by `Handle` from here and closed on drop.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|error| format!("CreateToolhelp32Snapshot failed ({})", error.code().0))?;
    let snapshot = Handle(snapshot);
    let mut entry = PROCESSENTRY32W {
        dwSize: u32::try_from(size_of::<PROCESSENTRY32W>()).unwrap_or(u32::MAX),
        ..PROCESSENTRY32W::default()
    };
    let mut pids = Vec::new();
    let at: HANDLE = snapshot.0;
    // SAFETY: the snapshot handle is live; `entry` is a live, writable PROCESSENTRY32W whose
    // `dwSize` is its size, as both calls require.
    let mut next = unsafe { Process32FirstW(at, &raw mut entry) };
    while next.is_ok() {
        pids.push(entry.th32ProcessID);
        // SAFETY: as above.
        next = unsafe { Process32NextW(at, &raw mut entry) };
    }
    Ok(pids)
}
