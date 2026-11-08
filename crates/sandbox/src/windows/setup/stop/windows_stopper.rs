//! The stop's real [`Stopper`]: the record in `HKLM\SOFTWARE\Clauseo\Sandbox` (64-bit view), the
//! admin copy's path under `FOLDERID_ProgramFiles`, a Toolhelp process snapshot with each
//! process's user from the kernel's process list (`WTSEnumerateProcessesExW`), its token and its
//! owner, `NtSuspendProcess` and `TerminateProcess` through the handle that user was read
//! through, the set-up lock, and the Event Log.
#![expect(
    unsafe_code,
    reason = "FFI to the Toolhelp snapshot, the WTS process list, OpenProcess and TerminateProcess"
)]

use std::collections::BTreeMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::time::Duration;

use windows::Wdk::System::Threading::{
    NtQueryInformationProcess, ProcessBasicInformation, ProcessImageFileName,
};
use windows::Win32::Foundation::{HANDLE, NTSTATUS, UNICODE_STRING, WAIT_OBJECT_0};
use windows::Win32::Storage::FileSystem::READ_CONTROL;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::RemoteDesktop::{
    WTS_CURRENT_SERVER_HANDLE, WTS_PROCESS_INFO_EXW, WTSEnumerateProcessesExW, WTSFreeMemoryExW,
    WTSTypeProcessInfoLevel1,
};
use windows::Win32::System::SystemInformation::{GetSystemTimeAsFileTime, GetTickCount64};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_ACCESS_RIGHTS, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SUSPEND_RESUME, PROCESS_SYNCHRONIZE,
    PROCESS_TERMINATE, QueryFullProcessImageNameW, Sleep, TerminateProcess, WaitForSingleObject,
};
use windows::core::PWSTR;

use windows::Win32::Networking::BackgroundIntelligentTransferService::IBackgroundCopyJob;

use super::windows_bits::{self, Com};
use super::{Failed, Image, Pool, Process, StopExit, Stopper, stop};
use crate::windows::acl::object::Handle;
use crate::windows::admin_copy::{self, os};
use crate::windows::refresh::event_log;
use crate::windows::setup::lock::{Held, SETUP_MUTEX, SetupMutex};
use crate::windows::setup::machine::MachineError;
use crate::windows::setup::owner::this_process;
use crate::windows::setup::preflight::sid_text;
use crate::windows::setup::processes::{DebugPrivilege, check_pool, owner_of, user_of};
use crate::windows::setup::record_write::{self, RECORD_KEY};
use crate::windows::sids::Sid;

/// The exit code a stopped process gets.
const STOPPED_EXIT: u32 = 1;
/// The pause before the next pass after a termination failed, in milliseconds.
const PAUSE_MS: u32 = 250;
/// `WTS_ANY_SESSION`: every session's processes.
const ANY_SESSION: u32 = 0xFFFF_FFFE;
/// `WTS_PROCESS_INFO_LEVEL_1`: each process with its user's SID.
const INFO_LEVEL_1: u32 = 1;

windows::core::link!("ntdll.dll" "system" fn NtSuspendProcess(process: HANDLE) -> NTSTATUS);
/// How long the stop waits for the set-up lock (to reserve its number, mark the users, or write
/// its completion), in milliseconds: well inside the task's 2-minute limit.
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
    handles: Opened,
    /// The set-up lock, while held.
    held: Option<Held>,
    /// `GetTickCount64` when the stop began.
    began_ms: u64,
    /// The last BITS list's jobs, for [`Stopper::cancel_job`]; dropped before [`Self::com`].
    jobs: Vec<IBackgroundCopyJob>,
    /// COM on this thread, once the BITS jobs are listed.
    com: Option<Com>,
}

impl WindowsStopper {
    pub(super) fn new() -> WindowsStopper {
        WindowsStopper {
            _debug: DebugPrivilege::enable(),
            handles: Vec::new(),
            held: None,
            // SAFETY: GetTickCount64 only reads the tick count.
            began_ms: unsafe { GetTickCount64() },
            jobs: Vec::new(),
            com: None,
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
        // The check's own refusal names the SID; the Application log is readable by every
        // local user, so the line says only that one failed.
        check_pool(&pool.users, &pool.owner).map_err(|error| match error {
            MachineError::Other(_) => "a SID PoolSids lists failed the pool-user check".to_owned(),
            error @ (MachineError::Os { .. }
            | MachineError::LockHeld
            | MachineError::Unsupported(_)
            | MachineError::ProfileChanged(_)) => error.to_string(),
        })
    }

    fn snapshot(&mut self) -> Result<Vec<Process>, Failed> {
        self.handles.clear();
        let (seen, handles) = processes()?;
        self.handles = handles;
        Ok(seen)
    }

    fn own_pid(&mut self) -> u32 {
        // SAFETY: GetCurrentProcessId only reads this process's id.
        unsafe { GetCurrentProcessId() }
    }

    fn suspend(&mut self, pid: u32) -> Result<(), Failed> {
        let Some((_, handle)) = self.handles.iter().find(|(at, _)| *at == pid) else {
            return Err("not opened for suspension".to_owned());
        };
        // SAFETY: the handle is open on the process whose user was read; one opened without
        // PROCESS_SUSPEND_RESUME gets an error status back, nothing more.
        let status = unsafe { NtSuspendProcess(handle.0) };
        if status.is_ok() {
            Ok(())
        } else {
            Err(format!("NtSuspendProcess failed ({:#010x})", status.0))
        }
    }

    fn terminate(&mut self, pid: u32, wait: Duration) -> Result<(), Failed> {
        let Some((_, handle)) = self.handles.iter().find(|(at, _)| *at == pid) else {
            return Err("not opened for termination".to_owned());
        };
        // SAFETY: the handle is open with PROCESS_TERMINATE on the process whose user was read.
        unsafe { TerminateProcess(handle.0, STOPPED_EXIT) }
            .map_err(|error| format!("TerminateProcess failed ({})", error.code().0))?;
        let wait_ms = u32::try_from(wait.as_millis()).unwrap_or(u32::MAX);
        // SAFETY: the handle is open with PROCESS_SYNCHRONIZE; the wait is bounded.
        if unsafe { WaitForSingleObject(handle.0, wait_ms) } == WAIT_OBJECT_0 {
            Ok(())
        } else {
            Err("it hadn't exited when the wait ran out".to_owned())
        }
    }

    fn elapsed(&mut self) -> Duration {
        // SAFETY: GetTickCount64 only reads the tick count.
        let now = unsafe { GetTickCount64() };
        Duration::from_millis(now.saturating_sub(self.began_ms))
    }

    fn pause(&mut self) {
        // SAFETY: a plain wait of this thread.
        unsafe { Sleep(PAUSE_MS) };
    }

    fn bits_jobs(&mut self) -> Result<Vec<Option<Sid>>, Failed> {
        self.jobs.clear();
        self.com.get_or_insert_with(Com::new);
        let (jobs, owners) = windows_bits::jobs()?.into_iter().unzip();
        self.jobs = jobs;
        Ok(owners)
    }

    fn cancel_job(&mut self, at: usize) -> Result<(), Failed> {
        let job = self.jobs.get(at).ok_or_else(|| "no such job".to_owned())?;
        windows_bits::cancel(job)
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

    fn now(&mut self) -> u64 {
        // SAFETY: GetSystemTimeAsFileTime only reads the clock.
        let now = unsafe { GetSystemTimeAsFileTime() };
        u64::from(now.dwHighDateTime) << 32 | u64::from(now.dwLowDateTime)
    }

    fn read_stop_value(&mut self, name: &'static str) -> Result<Option<u64>, Failed> {
        record_write::read_stop_value(RECORD_KEY, name).map_err(|error| error.to_string())
    }

    fn write_stop_value(&mut self, name: &'static str, value: u64) -> Result<(), Failed> {
        record_write::write_stop_value(RECORD_KEY, name, value).map_err(|error| error.to_string())
    }

    fn log(&mut self, error: bool, line: &str) {
        event_log(error, line);
    }
}

/// The handles that can suspend and terminate a process, by pid.
pub(super) type Opened = Vec<(u32, Handle)>;

/// Every running process, as [`Process`] says, with the handles that can terminate one, by pid.
///
/// Each pid of a Toolhelp snapshot is opened, its token's user read through that handle, and
/// its owner when the token can't be read, and its image when neither can. One that has already
/// exited is dropped. Then the kernel's process list gives each process's user and session: it
/// is read only once every handle is open, and a pid isn't reused while a handle to its process
/// is open, so each entry for an opened pid is the process that handle is on. A process the
/// list names but the snapshot didn't (started since) is added with no handle, so a pool
/// user's keeps the passes going. A list that can't be read marks every process
/// `list_failed`: the token then says whose each is, and one it can't read is unknown, unless
/// its owner is a pool user (`pick::user_of`).
pub(super) fn processes() -> Result<(Vec<Process>, Opened), Failed> {
    let mut seen = Vec::new();
    // Every handle stays open until the list is read, the query-only ones too.
    let mut held = Vec::new();
    let mut exited = Vec::new();
    for pid in snapshot_pids()? {
        let Some((process, handle)) = look_at(pid) else {
            exited.push(pid);
            continue;
        };
        seen.push(process);
        if let Some(handle) = handle {
            held.push((pid, handle));
        }
    }
    match listed_users() {
        Ok(listed) => {
            for process in &mut seen {
                if let Some((user, session)) = listed.get(&process.pid) {
                    process.listed.clone_from(user);
                    process.session = Some(*session);
                }
            }
            for (pid, (listed, session)) in listed {
                let known = exited.contains(&pid) || seen.iter().any(|p| p.pid == pid);
                if !known {
                    seen.push(Process {
                        pid,
                        listed,
                        session: Some(session),
                        ..Process::default()
                    });
                }
            }
        }
        Err(why) => {
            tracing::warn!(%why, "the process list's users couldn't be read");
            for process in &mut seen {
                process.list_failed = true;
            }
        }
    }
    let can_stop: Vec<u32> = seen.iter().filter(|p| p.can_stop).map(|p| p.pid).collect();
    held.retain(|(pid, _)| can_stop.contains(pid));
    Ok((seen, held))
}

/// `pid` opened and read, with its handle; `None` when it has already exited. A process that
/// can't be opened at all is seen with no user and no handle.
pub(super) fn look_at(pid: u32) -> Option<(Process, Option<Handle>)> {
    let stop = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE;
    let tries: [(PROCESS_ACCESS_RIGHTS, bool); 3] = [
        (
            stop | PROCESS_SUSPEND_RESUME | PROCESS_ACCESS_RIGHTS(READ_CONTROL.0),
            true,
        ),
        (stop, true),
        (PROCESS_QUERY_LIMITED_INFORMATION, false),
    ];
    let opened = tries.iter().find_map(|(access, can_stop)| {
        // SAFETY: OpenProcess takes plain values; the handle is owned by `Handle` from here.
        let handle = unsafe { OpenProcess(*access, false, pid) }.ok()?;
        Some((Handle(handle), *can_stop))
    });
    let Some((handle, can_stop)) = opened else {
        let process = Process {
            pid,
            ..Process::default()
        };
        return Some((process, None));
    };
    // SAFETY: a zero wait on a live handle; one without SYNCHRONIZE fails, and is kept.
    if can_stop && unsafe { WaitForSingleObject(handle.0, 0) } == WAIT_OBJECT_0 {
        return None;
    }
    let token = user_of(&handle);
    let owner = if token.is_none() {
        owner_of(&handle)
    } else {
        None
    };
    let image = if token.is_none() && owner.is_none() {
        image_of(&handle)
    } else {
        Image::Unread
    };
    let process = Process {
        pid,
        token,
        owner,
        can_stop,
        image,
        ..Process::default()
    };
    Some((process, Some(handle)))
}

/// `handle`'s image, through query-limited access, which a protected process grants. The
/// kernel's own name for it (`ProcessImageFileName`) first: empty for a process with no image
/// file, which is [`Image::Missing`] only when [`subsystem`] says it isn't a subsystem (pico)
/// process, which has no image file either. Then its Win32 path; a path that can't be read is
/// [`Image::Failed`].
fn image_of(handle: &Handle) -> Image {
    // A UNICODE_STRING, then its text: room for the longest NT path.
    let mut buffer = vec![0u64; 8_200];
    let size = u32::try_from(buffer.len() * 8).unwrap_or(u32::MAX);
    let mut written = 0u32;
    // SAFETY: the handle is live with PROCESS_QUERY_LIMITED_INFORMATION; `buffer` is live,
    // writable and aligned for a UNICODE_STRING, `size` bytes long; `written` is a live
    // out-pointer.
    let status = unsafe {
        NtQueryInformationProcess(
            handle.0,
            ProcessImageFileName,
            buffer.as_mut_ptr().cast::<c_void>(),
            size,
            &raw mut written,
        )
    };
    if status.is_err() {
        return Image::Failed;
    }
    // SAFETY: on success the call wrote a UNICODE_STRING at the buffer's start.
    let name = unsafe { &*buffer.as_ptr().cast::<UNICODE_STRING>() };
    if name.Length == 0 {
        return match subsystem(handle) {
            Some(false) => Image::Missing,
            Some(true) => Image::Subsystem,
            None => Image::Failed,
        };
    }
    let mut path = vec![0u16; 32_768];
    let mut length = u32::try_from(path.len()).unwrap_or(u32::MAX);
    // SAFETY: the handle is live; `path` is a writable buffer of `length` units, and
    // `length` a live in-out pointer.
    let read = unsafe {
        QueryFullProcessImageNameW(
            handle.0,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &raw mut length,
        )
    };
    let units = usize::try_from(length)
        .ok()
        .and_then(|length| path.get(..length));
    match (read, units) {
        (Ok(()), Some(units)) if !units.is_empty() => Image::Path(String::from_utf16_lossy(units)),
        _ => Image::Failed,
    }
}

/// `PROCESS_EXTENDED_BASIC_INFORMATION`, declared here so the `windows` crate's
/// `Win32_System_Kernel` and `Wdk_System_SystemServices` aren't needed (as
/// `owner/process.rs` declares the basic one); the same `repr(C)` layout.
#[repr(C)]
#[derive(Default)]
struct ExtendedBasicInformation {
    size: usize,
    exit_status: i32,
    peb_base_address: usize,
    affinity_mask: usize,
    base_priority: i32,
    unique_process_id: usize,
    inherited_from_unique_process_id: usize,
    flags: u32,
}

/// `PROCESS_EXTENDED_BASIC_INFORMATION`'s `IsSubsystemProcess` bit (after `IsProtectedProcess`,
/// `IsWow64Process`, `IsProcessDeleting`, `IsCrossSessionCreate`, `IsFrozen`, `IsBackground`,
/// `IsStronglyNamed` and `IsSecureProcess`): set for a WSL 1 (pico) process.
const IS_SUBSYSTEM_PROCESS: u32 = 1 << 8;

/// Whether `handle`'s process is a subsystem (pico) process, from
/// `NtQueryInformationProcess(ProcessBasicInformation)` with the extended struct, which
/// query-limited access reads; `None` when the query fails or doesn't fill the extended struct.
fn subsystem(handle: &Handle) -> Option<bool> {
    let size = size_of::<ExtendedBasicInformation>();
    let mut info = ExtendedBasicInformation {
        size,
        ..ExtendedBasicInformation::default()
    };
    let length = u32::try_from(size).ok()?;
    let mut written = 0u32;
    // SAFETY: the handle is live with PROCESS_QUERY_LIMITED_INFORMATION; `info` is a live,
    // writable struct of `length` bytes with PROCESS_EXTENDED_BASIC_INFORMATION's layout, its
    // `size` set as the call requires; `written` is a live out-pointer.
    let status = unsafe {
        NtQueryInformationProcess(
            handle.0,
            ProcessBasicInformation,
            (&raw mut info).cast::<c_void>(),
            length,
            &raw mut written,
        )
    };
    // A kernel that filled only the basic struct leaves the flags unset: not an answer.
    if status.is_err() || written != length || info.size != size {
        return None;
    }
    Some(info.flags & IS_SUBSYSTEM_PROCESS != 0)
}

/// The kernel's process list (`WTSEnumerateProcessesExW`, level 1, every session): each pid
/// with its user's SID (`None` where the list names none) and its session, leaving out a
/// process with no thread (one that has exited, kept by a handle). The list reads each token as
/// this process: a token whose DACL shuts out SYSTEM gets no SID (CI run 42416074600), and its
/// process's owner names it instead (`pick::user_of`).
pub(crate) fn listed_users() -> Result<BTreeMap<u32, (Option<Sid>, u32)>, Failed> {
    let mut level = INFO_LEVEL_1;
    let mut info = PWSTR::null();
    let mut count = 0u32;
    // SAFETY: this machine's server handle; `level`, `info` and `count` are live
    // out-pointers; the list is freed by `List` once written.
    unsafe {
        WTSEnumerateProcessesExW(
            Some(WTS_CURRENT_SERVER_HANDLE),
            &raw mut level,
            ANY_SESSION,
            &raw mut info,
            &raw mut count,
        )
    }
    .map_err(|error| format!("WTSEnumerateProcessesExW failed ({})", error.code().0))?;
    let list = List(info, count);
    let entries: &[WTS_PROCESS_INFO_EXW] = if list.0.is_null() {
        &[]
    } else {
        // SAFETY: level 1 wrote `count` WTS_PROCESS_INFO_EXW entries at `info`, aligned and
        // live until `list` drops, after this slice's last use.
        unsafe {
            std::slice::from_raw_parts(
                list.0.0.cast::<WTS_PROCESS_INFO_EXW>(),
                usize::try_from(count).unwrap_or(0),
            )
        }
    };
    let mut users = BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.NumberOfThreads > 0) {
        let user = if entry.pUserSid.0.is_null() {
            None
        } else {
            // SAFETY: a non-null pUserSid is a valid SID inside the live list.
            unsafe { sid_text(entry.pUserSid) }.ok().flatten()
        };
        users.insert(entry.ProcessId, (user, entry.SessionId));
    }
    drop(list);
    Ok(users)
}

/// A level-1 process list from `WTSEnumerateProcessesExW` and its entry count, freed on drop.
struct List(PWSTR, u32);

impl Drop for List {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        // SAFETY: the list WTSEnumerateProcessesExW wrote at level 1, with its count, freed once.
        let freed = unsafe { WTSFreeMemoryExW(WTSTypeProcessInfoLevel1, self.0.0.cast(), self.1) };
        if let Err(error) = freed {
            tracing::debug!(code = error.code().0, "WTSFreeMemoryExW failed");
        }
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
